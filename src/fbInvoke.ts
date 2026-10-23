// fbInvoke.ts — 复用 api.ts 的结构化克隆净化 invoke（api.ts 未导出，此处薄转发）。
export async function invoke<T>(channel: string, ...args: unknown[]): Promise<T> {
    const clean = args.map((a) => {
        if (a === undefined || a === null) return a;
        try {
            return JSON.parse(JSON.stringify(a)) as unknown;
        } catch {
            return a;
        }
    });
    return (window as any).kamucl.invoke(channel, ...clean) as Promise<T>;
}
