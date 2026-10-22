//! A tool call's arguments, checked against the input schema the tool publishes before the tool
//! runs or opens anything. Clients, and the models behind them, build their calls from that schema;
//! a server that took what it does not declare would ignore a misspelled argument (`descripton`,
//! `components[0].runOn`) or a stray one (`type` on `delete_resource`) in silence, and the agent would
//! report a change that never happened, or make one it never meant.
//!
//! The schemas use a small subset of JSON Schema 2020-12, and this checks all of it: `type` (one
//! or a list), `enum`, `properties`, `required`, `additionalProperties: false`, `items`, `minimum`
//! and `maximum`, besides `description`, which states nothing to check. A test walks every
//! published schema and fails on any other keyword, so that no schema declares a rule the gate
//! would not enforce. As JSON Schema says, a number is an `integer` when it has no fractional part:
//! `30.0` is one, `30.5` is not.
//!
//! What that subset cannot say (name a system by `id` or `name`; `before` with observations) is in
//! `rules`, stated in each tool's description and checked here too, still before anything opens.
//! Only what needs the inventory is checked later, where the system is found: that an `id` and a
//! `name` given together name the same system.

use serde_json::{Map, Value};

/// What is wrong with `arguments` against `schema`, a tool's input schema: one sentence per
/// problem, each naming where it is (`components[1].runsOn`) and what is accepted there. Empty
/// when they match.
pub(super) fn problems(schema: &Value, arguments: &Map<String, Value>) -> Vec<String> {
    let mut found = Vec::new();
    properties(schema, arguments, "", &mut found);
    found
}

fn check(schema: &Value, value: &Value, path: &str, found: &mut Vec<String>) {
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    if !types.is_empty() && !types.iter().any(|kind| is(value, kind)) {
        found.push(format!(
            "`{path}` must be {}, not {}.",
            either(&types),
            what(value)
        ));
        return;
    }
    if let Some(choices) = schema.get("enum").and_then(Value::as_array) {
        if !choices.contains(value) {
            let choices: Vec<String> = choices.iter().map(Value::to_string).collect();
            found.push(format!(
                "`{path}` must be one of {}, not {value}.",
                choices.join(", ")
            ));
        }
    }
    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema
            .get("minimum")
            .filter(|bound| bound.as_f64().is_some_and(|minimum: f64| number < minimum))
        {
            found.push(format!("`{path}` must be at least {minimum}, not {value}."));
        }
        if let Some(maximum) = schema
            .get("maximum")
            .filter(|bound| bound.as_f64().is_some_and(|maximum: f64| number > maximum))
        {
            found.push(format!("`{path}` must be at most {maximum}, not {value}."));
        }
    }
    match value {
        Value::Object(object) => properties(schema, object, path, found),
        Value::Array(items) => {
            if let Some(item) = schema.get("items") {
                for (index, value) in items.iter().enumerate() {
                    check(item, value, &format!("{path}[{index}]"), found);
                }
            }
        }
        _ => {}
    }
}

/// The object's required properties, each declared one against its schema, and, where the schema
/// closes the object (`additionalProperties: false`), every property it does not declare.
fn properties(schema: &Value, object: &Map<String, Value>, path: &str, found: &mut Vec<String>) {
    let declared = schema.get("properties").and_then(Value::as_object);
    for key in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !object.contains_key(key) {
            found.push(format!("`{}` is required.", at(path, key)));
        }
    }
    let closed = schema.get("additionalProperties") == Some(&Value::Bool(false));
    for (key, value) in object {
        match declared.and_then(|declared| declared.get(key)) {
            Some(property) => check(property, value, &at(path, key), found),
            None if closed => found.push(unknown(path, key, declared)),
            None => {}
        }
    }
}

fn at(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

/// A property the schema does not declare: what is accepted there instead and, when one of those
/// is a likely misspelling of it, which.
fn unknown(path: &str, key: &str, declared: Option<&Map<String, Value>>) -> String {
    let names: Vec<&str> = declared
        .map(|declared| declared.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let place = if path.is_empty() {
        "an argument of this tool".to_owned()
    } else {
        format!("a field of `{path}`")
    };
    let accepted = if names.is_empty() {
        "it takes none".to_owned()
    } else {
        format!("it takes {}", names.join(", "))
    };
    let hint = closest(key, &names)
        .map(|name| format!(" Did you mean `{name}`?"))
        .unwrap_or_default();
    format!("`{}` is not {place}; {accepted}.{hint}", at(path, key))
}

/// The accepted name `key` most likely misspells: the same but for case, or else the only nearest
/// one within two edits (one, for a name of four letters or fewer).
fn closest<'a>(key: &str, names: &[&'a str]) -> Option<&'a str> {
    let key = key.to_lowercase();
    if let Some(name) = names.iter().find(|name| name.to_lowercase() == key) {
        return Some(name);
    }
    let allowed = if key.chars().count() <= 4 { 1 } else { 2 };
    let distances: Vec<(usize, &str)> = names
        .iter()
        .map(|name| (edits(&key, &name.to_lowercase()), *name))
        .filter(|(distance, _)| *distance <= allowed)
        .collect();
    let nearest = distances.iter().map(|(distance, _)| *distance).min()?;
    match distances
        .iter()
        .filter(|(distance, _)| *distance == nearest)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [(_, name)] => Some(name),
        _ => None,
    }
}

/// The Levenshtein distance between two names: how many characters to insert, delete or replace.
fn edits(one: &str, other: &str) -> usize {
    let other: Vec<char> = other.chars().collect();
    let mut previous: Vec<usize> = (0..=other.len()).collect();
    for (row, left) in one.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right) in other.iter().enumerate() {
            let replace = previous[column] + usize::from(left != *right);
            current.push(
                replace
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[other.len()]
}

fn is(value: &Value, kind: &str) -> bool {
    match kind {
        "string" => value.is_string(),
        "integer" => value.as_f64().is_some_and(|number| number.fract() == 0.0),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => false,
    }
}

/// The types a value may have, as a sentence says them.
fn either(types: &[&str]) -> String {
    let named: Vec<&str> = types
        .iter()
        .map(|kind| match *kind {
            "string" => "a string",
            "integer" => "a whole number",
            "number" => "a number",
            "boolean" => "true or false",
            "object" => "an object",
            "array" => "a list",
            "null" => "null",
            other => other,
        })
        .collect();
    named.join(" or ")
}

/// What a value is, as a sentence says it: scalars as they were sent, the rest by their type.
fn what(value: &Value) -> String {
    match value {
        Value::String(_) => "a string".to_owned(),
        Value::Array(_) => "a list".to_owned(),
        Value::Object(_) => "an object".to_owned(),
        scalar => scalar.to_string(),
    }
}

/// A JSON number as a whole number DevHQ can hold: one without a fractional part, `30` or `30.0`,
/// within the range of i64. Every integer a schema declares is bounded well inside it, so after
/// `problems` this never fails on an argument the schema accepted.
pub(super) fn whole(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        let number = value.as_f64()?;
        // i64::MAX as f64 is 2^63, one past the largest i64.
        (number.fract() == 0.0 && number >= i64::MIN as f64 && number < i64::MAX as f64)
            .then_some(number as i64)
    })
}

/// The tools that name one system, by `id` or `name` (and `type`, unless the tool says which).
const SYSTEM_TOOLS: [&str; 11] = [
    "get_system",
    "history",
    "record_observation",
    "open_incident",
    "record_event",
    "save_check",
    "set_check_paused",
    "remove_check",
    "run_check",
    "delete_application",
    "delete_resource",
];

/// What breaks the rules between `tool`'s arguments that its schema cannot state and its
/// description does. Checked once the arguments match the schema, so their types are right.
pub(super) fn rules(tool: &str, arguments: &Map<String, Value>) -> Vec<String> {
    let given = |key: &str| arguments.get(key).is_some_and(|value| !value.is_null());
    let mut found = Vec::new();
    if SYSTEM_TOOLS.contains(&tool) && !given("id") && !given("name") {
        found.push("Name the system by `id` or by `name`.".to_owned());
    }
    match tool {
        "read_copies" => {
            let system = given("type") || given("id") || given("name");
            if given("referenceId") && system {
                found.push("Name either one reference (`referenceId`) or one system (`type` and `id` or `name`), not both.".to_owned());
            } else if (given("id") || given("name")) && !given("type") {
                found.push(
                    "`id` and `name` name a system only with its `type`: application or resource."
                        .to_owned(),
                );
            } else if given("type") && !given("id") && !given("name") {
                found.push("Name the system by `id` or by `name`.".to_owned());
            }
        }
        "history" => {
            let paged = matches!(
                arguments.get("records").and_then(Value::as_str),
                Some("observations" | "events")
            );
            if paged && !given("before") {
                found.push("`before` is required for observations and events: the id of the oldest one already shown (get_system shows them).".to_owned());
            }
        }
        "save_application" | "save_resource" => {
            let editing = given("id");
            if editing {
                // Present, even as null: null says the system had no change logged.
                if !arguments.contains_key("basedOn") {
                    found.push("`basedOn` is required to edit: the latestChange that list_systems or get_system gave for this system (null when they gave null). DevHQ refuses an edit if the system changed after it was read.".to_owned());
                }
            } else {
                // Adding needs what editing keeps; the transaction says it again, in the same words.
                let needed: [(&str, &str); 2] = if tool == "save_application" {
                    [
                        ("name", "name is required to add an application."),
                        ("lifecycle", "lifecycle is required to add an application: development, testing, production or archived."),
                    ]
                } else {
                    [
                        ("name", "name is required to add a resource."),
                        (
                            "kind",
                            "kind is required to add a resource: server or workstation.",
                        ),
                    ]
                };
                for (key, message) in needed {
                    if !given(key) {
                        found.push(message.to_owned());
                    }
                }
                for key in ["basedOn", "removeComponents", "removeReferences"] {
                    if arguments.contains_key(key) {
                        found.push(format!("`{key}` is only for an edit, with `id`."));
                    }
                }
            }
            merged_list(
                arguments,
                ("components", "removeComponents"),
                ("component", &["name", "kind"]),
                editing,
                &mut found,
            );
            merged_list(
                arguments,
                ("references", "removeReferences"),
                ("reference", &["kind", "target"]),
                editing,
                &mut found,
            );
            hidden_characters(arguments, &mut found);
        }
        _ => {}
    }
    found
}

/// The rules of a list that an agent's call merges into what the system has (`components`,
/// `references`) and of the ids it removes from it: an item without `id` is a new one and needs
/// what it cannot keep (`needs`); an item with `id` changes that one, so only an edit has them, and
/// once each; an id is removed once, and never also changed.
fn merged_list(
    arguments: &Map<String, Value>,
    (list, removals): (&str, &str),
    (noun, needs): (&str, &[&str]),
    editing: bool,
    found: &mut Vec<String>,
) {
    let mut named: Vec<&str> = Vec::new();
    let items = arguments
        .get(list)
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for (index, item) in items.enumerate() {
        let Some(item) = item.as_object() else {
            continue;
        };
        match item.get("id").and_then(Value::as_str) {
            Some(_) if !editing => found.push(format!(
                "`{list}[{index}].id`: everything a new system has is new, so its {noun}s take no id."
            )),
            Some(id) if named.contains(&id) => found.push(format!(
                "`{list}[{index}].id`: {id} is named twice; change each {noun} once."
            )),
            Some(id) => named.push(id),
            None => {
                for key in needs {
                    if !item.contains_key(*key) {
                        found.push(format!(
                            "`{list}[{index}].{key}` is required for a new {noun}, one without id."
                        ));
                    }
                }
            }
        }
    }
    let mut removed: Vec<&str> = Vec::new();
    let ids = arguments
        .get(removals)
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    for (index, id) in ids.filter_map(Value::as_str).enumerate() {
        if named.contains(&id) {
            found.push(format!(
                "`{removals}[{index}]`: {id} is also in `{list}`; change that {noun} or remove it, not both."
            ));
        }
        if removed.contains(&id) {
            found.push(format!("`{removals}[{index}]`: {id} is named twice."));
        }
        removed.push(id);
    }
}

/// Text an agent saves that would not read as it is kept: a control character (a tab, a NUL, a line
/// break anywhere but in a description…) or an invisible formatting one (Unicode's format
/// characters: marks that reverse the direction of text, zero-width spaces and joiners, a byte
/// order mark…). Either would make a name the person reads in DevHQ differ from the one kept, or
/// hide part of it. The window takes what the person types; agents write text nobody typed.
fn hidden_characters(arguments: &Map<String, Value>, found: &mut Vec<String>) {
    let mut check = |path: String, value: Option<&Value>, lines: bool| {
        let Some(text) = value.and_then(Value::as_str) else {
            return;
        };
        if let Some(character) = text.chars().find(|character| hidden(*character, lines)) {
            found.push(format!(
                "`{path}` holds U+{:04X}, a control or invisible formatting character: what DevHQ shows would not be what it keeps. Remove it.",
                u32::from(character)
            ));
        }
    };
    for key in ["name", "description", "version", "operatingSystem"] {
        check(key.to_owned(), arguments.get(key), key == "description");
    }
    for (list, keys) in [
        ("components", &["name"][..]),
        ("references", &["target", "label"][..]),
    ] {
        let items = arguments
            .get(list)
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for (index, item) in items.enumerate() {
            for key in keys {
                check(format!("{list}[{index}].{key}"), item.get(*key), false);
            }
        }
    }
}

/// Whether `character` hides or breaks what a text shows: a control character (line breaks and
/// tabs are fine where the text has `lines`), or a format character.
fn hidden(character: char, lines: bool) -> bool {
    if lines && matches!(character, '\n' | '\r' | '\t') {
        return false;
    }
    character.is_control() || format_character(character)
}

/// Unicode's format characters (general category Cf, Unicode 15.1), which Rust's standard library
/// does not name: invisible, they change how the text around them is shown.
fn format_character(character: char) -> bool {
    matches!(
        u32::from(character),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every keyword a published schema may use: what `check` enforces, and `description`.
    const KEYWORDS: [&str; 9] = [
        "type",
        "enum",
        "properties",
        "required",
        "additionalProperties",
        "items",
        "minimum",
        "maximum",
        "description",
    ];

    /// The problems of `arguments` against `schema`.
    fn against(schema: Value, arguments: Value) -> Vec<String> {
        problems(&schema, arguments.as_object().unwrap())
    }

    #[test]
    fn every_published_schema_uses_only_what_the_gate_checks() {
        fn walk(schema: &Value, path: &str, tool: &str) {
            let object = schema
                .as_object()
                .unwrap_or_else(|| panic!("{tool}: {path} is not a schema"));
            for keyword in object.keys() {
                assert!(
                    KEYWORDS.contains(&keyword.as_str()),
                    "{tool}: {path} uses `{keyword}`, which the gate does not check"
                );
            }
            let types: Vec<&str> = match &schema["type"] {
                Value::String(kind) => vec![kind.as_str()],
                Value::Array(kinds) => kinds.iter().map(|kind| kind.as_str().unwrap()).collect(),
                other => panic!("{tool}: {path} has no type: {other}"),
            };
            if types.contains(&"object") {
                // Every object is closed: nothing undeclared is ever taken in silence.
                assert_eq!(schema["additionalProperties"], false, "{tool}: {path}");
                let properties = schema["properties"].as_object().unwrap();
                for required in schema["required"].as_array().unwrap() {
                    assert!(
                        properties.contains_key(required.as_str().unwrap()),
                        "{tool}: {path} requires an undeclared {required}"
                    );
                }
                for (key, property) in properties {
                    walk(property, &format!("{path}.{key}"), tool);
                }
            }
            if types.contains(&"array") {
                walk(&schema["items"], &format!("{path}[]"), tool);
            }
            // Bounded well inside i64, so that `whole` reads every integer the schema accepts.
            if types.contains(&"integer") {
                let bound = |keyword: &str| {
                    schema[keyword]
                        .as_i64()
                        .unwrap_or_else(|| panic!("{tool}: {path} is an integer without {keyword}"))
                };
                assert!(bound("minimum") > i64::from(i32::MIN), "{tool}: {path}");
                assert!(bound("maximum") < i64::from(i32::MAX), "{tool}: {path}");
            }
        }
        let tools = super::super::tools();
        assert_eq!(tools.len(), 19);
        for tool in &tools {
            let name = tool["name"].as_str().unwrap();
            assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
            walk(&tool["inputSchema"], "arguments", name);
        }
    }

    #[test]
    fn names_each_problem_where_it_is() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "lifecycle": { "type": "string", "enum": ["development", "production"] },
                "version": { "type": ["string", "null"] },
                "minutes": { "type": "integer", "minimum": 1, "maximum": 60 },
                "components": { "type": "array", "items": { "type": "object", "properties": {
                    "name": { "type": "string" },
                    "runsOn": { "type": ["string", "null"] },
                }, "required": ["name"], "additionalProperties": false } },
            },
            "required": ["name"],
            "additionalProperties": false,
        });
        assert_eq!(
            against(
                schema.clone(),
                json!({ "name": "Shop", "version": null, "minutes": 60,
                        "components": [{ "name": "API", "runsOn": null }] })
            ),
            Vec::<String>::new()
        );
        let found = against(
            schema.clone(),
            json!({ "lifecycle": "live", "version": 2, "minutes": 0, "descripton": "x",
                    "components": [{ "name": "API", "runOn": "Host" }, { "runsOn": 1 }, "Web"] }),
        );
        let expected = [
            "`name` is required.",
            "`lifecycle` must be one of \"development\", \"production\", not \"live\".",
            "`version` must be a string or null, not 2.",
            "`minutes` must be at least 1, not 0.",
            "`components[0].runOn` is not a field of `components[0]`; it takes name, runsOn. Did you mean `runsOn`?",
            "`components[1].name` is required.",
            "`components[1].runsOn` must be a string or null, not 1.",
            "`components[2]` must be an object, not a string.",
        ];
        for problem in expected {
            assert!(
                found.iter().any(|item| item == problem),
                "{problem}\nnot in {found:#?}"
            );
        }
        assert!(
            found
                .iter()
                .any(|item| item.starts_with("`descripton` is not an argument of this tool;")),
            "{found:#?}"
        );
        assert_eq!(found.len(), expected.len() + 1, "{found:#?}");
        assert_eq!(
            against(schema, json!({ "name": "Shop", "minutes": 61 })),
            ["`minutes` must be at most 60, not 61."]
        );
    }

    /// JSON Schema 2020-12: an integer is a number without a fractional part, however written.
    #[test]
    fn a_whole_number_is_an_integer_however_it_is_written() {
        let schema = json!({ "type": "object", "properties": {
            "minutes": { "type": "integer", "minimum": 1, "maximum": 43200 },
        }, "required": [], "additionalProperties": false });
        for whole_number in [json!(30), json!(30.0), json!(3e1)] {
            assert_eq!(
                against(schema.clone(), json!({ "minutes": whole_number })),
                Vec::<String>::new(),
                "{whole_number}"
            );
            assert_eq!(whole(&whole_number), Some(30));
        }
        assert_eq!(
            against(schema.clone(), json!({ "minutes": 30.5 })),
            ["`minutes` must be a whole number, not 30.5."]
        );
        assert_eq!(
            against(schema, json!({ "minutes": "30" })),
            ["`minutes` must be a whole number, not a string."]
        );
        assert_eq!(whole(&json!(30.5)), None);
        assert_eq!(whole(&json!(1e19)), None);
        assert_eq!(whole(&json!(-5.0)), Some(-5));
    }

    fn saving(tool: &str, arguments: Value) -> Vec<String> {
        rules(tool, arguments.as_object().unwrap())
    }

    #[test]
    fn an_edit_says_what_it_read_and_names_what_it_removes() {
        let based = "`basedOn` is required to edit";
        let found = saving("save_application", json!({ "id": "shop", "version": "2" }));
        assert!(
            found.len() == 1 && found[0].starts_with(based),
            "{found:#?}"
        );
        let found = saving("save_resource", json!({ "id": "host", "name": "Host" }));
        assert!(
            found.len() == 1 && found[0].starts_with(based),
            "{found:#?}"
        );
        // Null is what to send for a system whose change log held nothing.
        assert!(saving("save_application", json!({ "id": "shop", "basedOn": null })).is_empty());
        assert!(saving(
            "save_application",
            json!({ "id": "shop", "basedOn": "c1",
                    "components": [{ "id": "api", "runsOn": null }, { "name": "Web", "kind": "frontend" }],
                    "removeComponents": ["db"], "references": [{ "id": "repo", "label": null }],
                    "removeReferences": ["docs"] })
        )
        .is_empty());

        let found = saving(
            "save_application",
            json!({ "id": "shop", "basedOn": "c1",
                    "components": [{ "id": "api" }, { "id": "api" }, { "name": "Web" }],
                    "removeComponents": ["api", "db", "db"],
                    "references": [{ "target": "https://shop.example/" }] }),
        );
        assert_eq!(
            found,
            [
                "`components[1].id`: api is named twice; change each component once.",
                "`components[2].kind` is required for a new component, one without id.",
                "`removeComponents[0]`: api is also in `components`; change that component or remove it, not both.",
                "`removeComponents[2]`: db is named twice.",
                "`references[0].kind` is required for a new reference, one without id.",
            ]
        );

        // What a new system has is all new: nothing to base it on, to name by id or to remove.
        let found = saving(
            "save_application",
            json!({ "name": "Shop", "lifecycle": "production", "basedOn": null,
                    "removeReferences": ["repo"],
                    "components": [{ "id": "api", "name": "API", "kind": "backend" }] }),
        );
        assert_eq!(
            found,
            [
                "`basedOn` is only for an edit, with `id`.",
                "`removeReferences` is only for an edit, with `id`.",
                "`components[0].id`: everything a new system has is new, so its components take no id.",
            ]
        );
        let found = saving(
            "save_resource",
            json!({ "name": "Box", "references": [{ "id": "logs", "kind": "logs", "target": "/var/log" }] }),
        );
        assert_eq!(
            found,
            [
                "kind is required to add a resource: server or workstation.",
                "`references[0].id`: everything a new system has is new, so its references take no id.",
            ]
        );
    }

    #[test]
    fn refuses_text_that_would_not_read_as_it_is_kept() {
        let found = saving(
            "save_application",
            json!({ "name": "Pass\u{202E}Guardian", "lifecycle": "development",
                    "description": "Two lines\nand a\ttab are fine here", "version": "1.0\u{200B}",
                    "components": [{ "name": "API\u{0007}", "kind": "backend" }],
                    "references": [{ "kind": "repository", "target": "https://x.example/\u{2066}", "label": "\u{FEFF}Main" }] }),
        );
        let held = |path: &str, code: &str| {
            format!("`{path}` holds U+{code}, a control or invisible formatting character: what DevHQ shows would not be what it keeps. Remove it.")
        };
        assert_eq!(
            found,
            [
                held("name", "202E"),
                held("version", "200B"),
                held("components[0].name", "0007"),
                held("references[0].target", "2066"),
                held("references[0].label", "FEFF"),
            ]
        );
        // A line break is fine only in a description.
        assert_eq!(
            saving(
                "save_resource",
                json!({ "name": "Box\nTwo", "kind": "server", "operatingSystem": "Linux\t" })
            ),
            [held("name", "000A"), held("operatingSystem", "0009")]
        );
        // Letters of any script, accents and emoji are text, not hidden characters.
        assert!(saving(
            "save_application",
            json!({ "name": "Ñandú — Финансы 財務 🚀", "lifecycle": "development" })
        )
        .is_empty());
    }

    #[test]
    fn suggests_only_a_likely_misspelling() {
        let names = ["id", "name", "description", "runsOn", "kind"];
        assert_eq!(closest("descripton", &names), Some("description"));
        assert_eq!(closest("Description", &names), Some("description"));
        assert_eq!(closest("runOn", &names), Some("runsOn"));
        assert_eq!(closest("ID", &names), Some("id"));
        // Nothing near enough, or two equally near: no guess.
        assert_eq!(closest("type", &names), None);
        assert_eq!(closest("verbose", &names), None);
        assert_eq!(closest("nd", &["id", "nid", "na"]), None);
        assert_eq!(edits("kitten", "sitting"), 3);
    }
}
