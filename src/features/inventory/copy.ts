import { useCallback, useEffect, useState } from 'react'

/** How long a confirmation such as «Path copied.» stays, in milliseconds. */
export const copiedFor = 3000

/**
 * Copies text to the clipboard and remembers how the last copy went, and of what (`key`, when
 * a view copies several things). A confirmation goes away by itself; a failure stays, since it
 * says what to do instead. `at` tells each click apart, so that copying again shows the
 * confirmation anew, with its full time.
 */
export function useCopy() {
  const [copied, setCopied] = useState<{ ok: boolean; at: number; key?: string } | null>(null)
  useEffect(() => {
    if (!copied?.ok) return
    const timer = setTimeout(() => setCopied(null), copiedFor)
    return () => clearTimeout(timer)
  }, [copied])
  const copy = useCallback(async (text: string, key?: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied({ ok: true, at: Date.now(), key })
    } catch {
      setCopied({ ok: false, at: Date.now(), key })
    }
  }, [])
  return { copied, copy }
}
