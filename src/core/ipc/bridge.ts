/** Typed bridge to the renderer preload contract exposed by the Tauri shim. */
export function invokeIpc<T>(channel: string, ...args: unknown[]): Promise<T> {
    // Vue reactive proxies cannot cross structured clone. Preserve the historical
    // JSON-cleaning behavior, including keeping top-level null and undefined values.
    const clean = args.map((arg) => {
        if (arg === undefined || arg === null) return arg;
        try {
            return JSON.parse(JSON.stringify(arg)) as unknown;
        } catch {
            return arg;
        }
    });
    return window.kamucl.invoke(channel, ...clean) as Promise<T>;
}

export function subscribeIpc<T>(channel: string, callback: (payload: T) => void): () => void {
    return window.kamucl.on(channel, (payload) => callback(payload as T));
}

export function sendIpc(channel: string, ...args: unknown[]): void {
    window.kamucl.send(channel, ...args);
}

export function platform(): string {
    return window.kamucl.platform;
}

export function filePathFromDrop(file: File): string {
    return window.kamucl.getFilePath(file);
}
