/**
 * 种子地图瓦片解码 Worker：base64 → ImageData → ImageBitmap 全部离开主线程。
 * 缩小视图跨刻度时会有几十片集中到达，原先在主线程逐片 atob + 建位图 + 重绘，
 * 是「一缩小就卡」的主要元凶；ImageBitmap 可 transfer，回传主线程零拷贝。
 */

type DecodeRequest = { token: number; encoded: string; width: number; height: number };
type DecodeResponse = { token: number; bitmap?: ImageBitmap; error?: string };

const post = (message: DecodeResponse, transfer?: Transferable[]) =>
    (self as unknown as { postMessage: (m: DecodeResponse, t?: Transferable[]) => void }).postMessage(
        message,
        transfer
    );

self.onmessage = (event: MessageEvent<DecodeRequest>) => {
    const { token, encoded, width, height } = event.data;
    void (async () => {
        try {
            const binary = atob(encoded);
            const expected = width * height * 4;
            if (binary.length !== expected) {
                throw new Error('The seed map backend returned an unexpected tile size.');
            }
            const pixels = new Uint8ClampedArray(binary.length);
            for (let i = 0; i < binary.length; i++) pixels[i] = binary.charCodeAt(i);
            const bitmap = await createImageBitmap(new ImageData(pixels, width, height), {
                colorSpaceConversion: 'none',
                premultiplyAlpha: 'none'
            });
            post({ token, bitmap }, [bitmap]);
        } catch (error) {
            post({ token, error: error instanceof Error ? error.message : String(error) });
        }
    })();
};
