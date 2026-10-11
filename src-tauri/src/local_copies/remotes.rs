//! A copy's remotes, as DevHQ keeps them and compares them.
//!
//! libgit2 gives a remote's URL as its config has it, credentials included
//! (`https://user:token@host/…`, `file://user:password@server/share/…`). DevHQ keeps only the forms
//! it knows, cleaned: http(s) without a user name, password, query or fragment; `ssh://` without a
//! password; `user@host:path`; `git://` and `file://` without a user name or password; and absolute
//! paths. Anything else (`ext::`, which is a command line, a remote helper's `name::address`, a
//! relative path) is kept by its name only, without its URL.
//!
//! Comparing a copy's remote with the repository its system declares needs both normalized, since
//! the real remote is often `git@github.com:me/shop.git` while the declared repository is
//! `https://github.com/me/shop`. Only that comparison normalizes; which systems declare the same
//! repository (`references::shared_repositories`) still compares URLs exactly.

use reqwest::Url;

/// The longest remote URL DevHQ keeps, as long as the longest URL anywhere else.
const MAX_URL: usize = 2048;

/// The URL as DevHQ keeps it, or None when it is not a form DevHQ knows how to clean.
pub(crate) fn sanitized(url: &str) -> Option<String> {
    let text = url.trim();
    if text.is_empty() || text.len() > MAX_URL || text.chars().any(char::is_control) {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let cleaned = if lower.starts_with("http://") || lower.starts_with("https://") {
        let mut parsed = Url::parse(text).ok()?;
        parsed.host_str()?;
        // A token is as often the user name as the password; neither is ever kept.
        parsed.set_username("").ok()?;
        parsed.set_password(None).ok()?;
        parsed.set_query(None);
        parsed.set_fragment(None);
        parsed.to_string()
    } else if lower.starts_with("ssh://") || lower.starts_with("git://") {
        let mut parsed = Url::parse(text).ok()?;
        parsed.host_str()?;
        // The user of an SSH URL (`git@`) names an account, not a secret; a password is one. The
        // git protocol has no accounts, so whatever stands before its @ can only be a secret.
        if lower.starts_with("git://") {
            parsed.set_username("").ok()?;
        }
        parsed.set_password(None).ok()?;
        parsed.set_query(None);
        parsed.set_fragment(None);
        parsed.to_string()
    } else if lower.starts_with("file://") {
        without_user_info(text)
    } else if absolute_path(text) {
        text.to_owned()
    } else if let Some((_, host, path)) = scp_like(text) {
        // `user@host:path` has no place for a password. Written as `user:password@host:path`,
        // the first colon ends the «host», and the rest still holds the password: refused.
        let after_at = path.find('@').map(|at| &path[at..]);
        if host.is_empty() || after_at.is_some_and(|rest| rest.contains(':')) {
            return None;
        }
        text.to_owned()
    } else {
        return None;
    };
    (cleaned.len() <= MAX_URL).then_some(cleaned)
}

/// A `file://` URL without what stands before the @ of its authority, a user name and a password
/// (`file://user:password@server/share/repo` keeps `file://server/share/repo`); the rest as written.
/// It is cut by hand because a WHATWG parser refuses user info in a file URL instead of exposing
/// it, and rewriting the path (percent-encoding, `\`) would change what the remote names. The
/// authority ends where a URL's does, at the first `/`, `\`, `?` or `#`; an @ after that is part
/// of the path.
fn without_user_info(text: &str) -> String {
    let (scheme, rest) = text.split_at("file://".len());
    let authority = rest
        .find(['/', '\\', '?', '#'])
        .map_or(rest, |end| &rest[..end]);
    match authority.rfind('@') {
        Some(at) => format!("{scheme}{}", &rest[at + 1..]),
        None => text.to_owned(),
    }
}

/// An absolute path on any system: `/…`, `C:\…`, `C:/…` or `\\server\share\…`.
fn absolute_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.starts_with('/')
        || text.starts_with(r"\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

/// `[user@]host:path`, the scp-like form Git reads as SSH: a colon before any slash, and no `::`
/// (a remote helper's `name::address`) or `://` (a URL). None for anything else, including a
/// Windows drive (`C:\…`), which is a path.
fn scp_like(text: &str) -> Option<(Option<&str>, &str, &str)> {
    if text.contains("://") || text.contains("::") || absolute_path(text) {
        return None;
    }
    let colon = text.find(':')?;
    if text[..colon].contains('/') {
        return None;
    }
    let (address, path) = (&text[..colon], &text[colon + 1..]);
    let (user, host) = match address.rfind('@') {
        Some(at) => (Some(&address[..at]), &address[at + 1..]),
        None => (None, address),
    };
    Some((user, host, path))
}

/// Where a remote or a repository URL points, compared regardless of protocol, user, port, case,
/// surrounding slashes and a trailing `.git`: (host, path). None for what cannot be compared (a
/// path on disk, a form DevHQ does not keep).
pub(crate) fn location(url: &str) -> Option<(String, String)> {
    let text = url.trim();
    let lower = text.to_ascii_lowercase();
    let (host, path) = if ["http://", "https://", "ssh://", "git://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
    {
        let parsed = Url::parse(text).ok()?;
        (parsed.host_str()?.to_owned(), parsed.path().to_owned())
    } else {
        let (_, host, path) = scp_like(text)?;
        (host.to_owned(), path.to_owned())
    };
    let path = path.trim_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some((host.to_ascii_lowercase(), path.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_known_forms_and_never_a_credential() {
        let cases: [(&str, Option<&str>); 32] = [
            (
                "https://github.com/me/shop.git",
                Some("https://github.com/me/shop.git"),
            ),
            (
                "https://ghp_token123@github.com/me/shop",
                Some("https://github.com/me/shop"),
            ),
            (
                "https://me:secret@gitlab.example:8443/me/shop.git?private_token=x#frag",
                Some("https://gitlab.example:8443/me/shop.git"),
            ),
            (
                "  HTTP://Example.COM/Me/Shop  ",
                Some("http://example.com/Me/Shop"),
            ),
            (
                "ssh://git@github.com/me/shop.git",
                Some("ssh://git@github.com/me/shop.git"),
            ),
            (
                "ssh://me:secret@host.example:2222/srv/shop.git",
                Some("ssh://me@host.example:2222/srv/shop.git"),
            ),
            (
                "git://git.example/shop.git",
                Some("git://git.example/shop.git"),
            ),
            (
                "git@github.com:me/shop.git",
                Some("git@github.com:me/shop.git"),
            ),
            ("host.example:shop.git", Some("host.example:shop.git")),
            // The git protocol has no accounts: a user name there is a secret too.
            (
                "git://token123@git.example/shop.git",
                Some("git://git.example/shop.git"),
            ),
            (
                "git://me:secret@git.example/shop.git",
                Some("git://git.example/shop.git"),
            ),
            ("file:///srv/git/shop.git", Some("file:///srv/git/shop.git")),
            (
                "file://server.example/share/shop",
                Some("file://server.example/share/shop"),
            ),
            // User info before a file URL's host, in every way a URL can end its authority; an @
            // in the path is a path.
            (
                "file://demo-user:demo-pass@server.example/share/repo",
                Some("file://server.example/share/repo"),
            ),
            (
                "FILE://token@server.example/share/repo",
                Some("FILE://server.example/share/repo"),
            ),
            (
                "file://me:p@ss@server.example/share/repo",
                Some("file://server.example/share/repo"),
            ),
            (
                r"file://me:secret@server.example\share\repo",
                Some(r"file://server.example\share\repo"),
            ),
            ("file://me:secret@/srv/shop", Some("file:///srv/shop")),
            ("file://me:secret@server", Some("file://server")),
            (
                "file:///srv/me@work/shop.git",
                Some("file:///srv/me@work/shop.git"),
            ),
            ("/srv/git/shop.git", Some("/srv/git/shop.git")),
            (r"C:\git\shop.git", Some(r"C:\git\shop.git")),
            ("D:/git/shop", Some("D:/git/shop")),
            (r"\\nas\git\shop.git", Some(r"\\nas\git\shop.git")),
            // A password before the @ of the scp-like form.
            ("me:secret@host.example:shop.git", None),
            // A command line, and remote helpers.
            ("ext::ssh -i ~/.ssh/key host.example %S 'shop'", None),
            ("hg::https://hg.example/shop", None),
            ("persistent-https://host.example/shop", None),
            // A relative path, or nothing to read.
            ("../shop.git", None),
            ("", None),
            ("https://host.example/sh\nop", None),
            ("https://", None),
        ];
        for (url, expected) in cases {
            assert_eq!(sanitized(url).as_deref(), expected, "{url}");
        }
        assert_eq!(
            sanitized(&format!("https://host.example/{}", "a".repeat(2048))),
            None
        );
    }

    #[test]
    fn compares_a_remote_with_a_declared_repository_wherever_both_point() {
        let shop = location("https://github.com/me/shop").unwrap();
        assert_eq!(shop, ("github.com".to_owned(), "me/shop".to_owned()));
        for same in [
            "git@github.com:me/shop.git",
            "git@github.com:/me/shop.git/",
            "ssh://git@github.com/me/shop.git",
            "ssh://git@GitHub.com:22/Me/Shop",
            "https://github.com/me/shop.git",
            "https://token@github.com/me/shop/",
            "git://github.com/me/shop",
        ] {
            assert_eq!(location(same), Some(shop.clone()), "{same}");
        }
        for other in [
            "git@github.com:me/blog.git",
            "git@gitlab.com:me/shop.git",
            // Azure DevOps names one repository differently over SSH and on the web.
            "git@ssh.dev.azure.com:v3/me/project/shop",
        ] {
            assert_ne!(location(other), Some(shop.clone()), "{other}");
        }
        assert_ne!(
            location("git@ssh.dev.azure.com:v3/me/project/shop"),
            location("https://dev.azure.com/me/project/_git/shop")
        );
        // Paths on disk and forms DevHQ does not keep are not compared.
        for nothing in [
            "/srv/git/shop.git",
            r"C:\git\shop",
            "file:///srv/git/shop.git",
            "ext::sh -c x",
            "https://github.com/",
        ] {
            assert_eq!(location(nothing), None, "{nothing}");
        }
    }
}
