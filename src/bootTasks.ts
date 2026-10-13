// Initial-render resources register before starting work. Requests created by watchers are drained too.
const pending = new Set<Promise<void>>()
let sealed = false
export function beginBootTask(): () => void {
  if (sealed) return () => {}
  let complete!: () => void
  const promise = new Promise<void>(resolve => { complete = resolve })
  pending.add(promise)
  return () => { pending.delete(promise); complete() }
}
export async function trackBootTask<T>(task: () => Promise<T>, budgetMs?: number): Promise<T> {
  const done = beginBootTask()
  // Optional remote enhancements keep loading after the usable fallback is painted.
  // Cached resources still finish inside the barrier, with no extra fixed delay.
  const timer = budgetMs === undefined ? undefined : setTimeout(done, budgetMs)
  try { return await task() } finally { clearTimeout(timer); done() }
}
export async function waitForBootTasks(): Promise<void> {
  while (pending.size) await Promise.all([...pending])
}
export function sealBootTasks(): void { sealed = true }
