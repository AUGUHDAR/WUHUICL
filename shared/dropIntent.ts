/** Internal text/image drags are editing gestures, never launcher imports. */
export function acceptsImportDrag(types: readonly string[], internal: boolean): boolean {
  return !internal && types.some(t => t === 'Files' || t === 'text/uri-list' || t === 'text/plain')
}
export function showsImportOverlay(types: readonly string[], internal: boolean): boolean {
  // Browsers do not expose plain text during protected dragover. Validate it on drop.
  return !internal && (types.includes('Files') || types.includes('text/uri-list'))
}
