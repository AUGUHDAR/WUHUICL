import { platform } from '../ipc/bridge';

/** Build an opaque URL for a local image that the main process has explicitly authorized. */
export function managedImageUrl(filePath: string): string {
    if (!filePath) return '';
    // Windows/WebView2 的自定义协议经 http://<scheme>.localhost 访问（Tauri/WRY 口径）
    const base =
        platform() === 'win32'
            ? 'http://kamucl-asset.localhost/image'
            : 'kamucl-asset://local/image';
    const url = new URL(base);
    url.searchParams.set('path', filePath);
    return url.toString();
}
