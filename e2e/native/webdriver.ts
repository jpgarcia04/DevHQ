/**
 * A minimal client for the W3C WebDriver protocol (https://www.w3.org/TR/webdriver2/), which
 * is JSON over HTTP: `fetch` is all it needs. It covers the commands the native walk uses and
 * nothing more, so it adds no dependency and every request it makes is visible here.
 */

/** The key under which WebDriver serializes a reference to an element. */
const ELEMENT_KEY = 'element-6066-11e4-a52e-4f735466cecf'

/** An element of the window, by the id the driver gave it. */
export type Element = string

/** An element as a script argument, which the window receives as the DOM node. */
export const reference = (element: Element) => ({ [ELEMENT_KEY]: element })

/** An error the remote end answered with, by its WebDriver error code. */
export class WebDriverError extends Error {
  readonly code: string

  constructor(code: string, message: string) {
    super(`${code}: ${message}`)
    this.code = code
  }
}

/** Errors that mean the element is not (or no longer) there yet, so a wait may try again. */
const transient = new Set([
  'no such element',
  'stale element reference',
  'element not interactable',
])

export const isTransient = (error: unknown) =>
  error instanceof WebDriverError && transient.has(error.code)

async function request<T>(url: string, method: string, body?: unknown): Promise<T> {
  const response = await fetch(url, {
    method,
    headers: body === undefined ? {} : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    // Creating a session launches DevHQ; nothing else should come close to this.
    signal: AbortSignal.timeout(120_000),
  })
  const text = await response.text()
  let payload: { value?: unknown }
  try {
    payload = JSON.parse(text) as { value?: unknown }
  } catch {
    throw new WebDriverError('invalid response', `${method} ${url} → ${response.status}: ${text}`)
  }
  const value = payload.value as { error?: string; message?: string } | null | undefined
  if (!response.ok || (value && typeof value === 'object' && 'error' in value)) {
    throw new WebDriverError(
      value?.error ?? `HTTP ${response.status}`,
      value?.message ?? `${method} ${url} failed`,
    )
  }
  return payload.value as T
}

/** Answers once the driver at `base` is up and ready to create a session. */
export async function ready(base: string) {
  const status = await request<{ ready?: boolean }>(`${base}/status`, 'GET')
  return status?.ready !== false
}

export class Session {
  readonly base: string
  readonly id: string

  private constructor(base: string, id: string) {
    this.base = base
    this.id = id
  }

  /** Starts a session, which launches the application the capabilities name. */
  static async create(base: string, capabilities: Record<string, unknown>) {
    const created = await request<{ sessionId: string }>(`${base}/session`, 'POST', {
      capabilities: { alwaysMatch: capabilities },
    })
    return new Session(base, created.sessionId)
  }

  private command<T>(method: 'GET' | 'POST' | 'DELETE', path: string, body?: unknown) {
    return request<T>(`${this.base}/session/${this.id}${path}`, method, body)
  }

  /** Every element that matches an XPath expression, in document order. */
  async findAll(xpath: string): Promise<Element[]> {
    const found = await this.command<Record<string, string>[]>('POST', '/elements', {
      using: 'xpath',
      value: xpath,
    })
    return found.map((item) => item[ELEMENT_KEY]!)
  }

  /** Clicks the element as a person would, through the window's own input events. */
  click(element: Element) {
    return this.command<null>('POST', `/element/${element}/click`, {})
  }

  /** Types into the element, key by key. */
  type(element: Element, text: string) {
    return this.command<null>('POST', `/element/${element}/value`, { text })
  }

  clear(element: Element) {
    return this.command<null>('POST', `/element/${element}/clear`, {})
  }

  /** The element's rendered text, as a person would read it. */
  text(element: Element) {
    return this.command<string>('GET', `/element/${element}/text`)
  }

  property<T>(element: Element, name: string) {
    return this.command<T>('GET', `/element/${element}/property/${name}`)
  }

  /** Runs a script in the window and returns what it returns. */
  execute<T>(script: string, args: unknown[] = []) {
    return this.command<T>('POST', '/execute/sync', { script, args })
  }

  /** The window as a PNG. */
  async screenshot() {
    return Buffer.from(await this.command<string>('GET', '/screenshot'), 'base64')
  }

  /** Ends the session. The driver may already have lost the application, so errors are ignored. */
  async end() {
    await this.command<null>('DELETE', '').catch(() => {})
  }
}
