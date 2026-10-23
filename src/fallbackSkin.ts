// 内置默认皮肤（原版史蒂夫，素材 src/assets/steve-default.png，与后端
// src-tauri/resources/skins/steve.png 同源）。账号无自定义皮肤时 3D 预览与
// 头像兜底渲染这张真实原版材质，而不是自绘占位像素画。
import steveUrl from './assets/steve-default.png';

function loadImage(url: string): Promise<HTMLImageElement> {
    return new Promise((resolve, reject) => {
        const image = new Image();
        image.onload = () => resolve(image);
        image.onerror = () => reject(new Error('默认皮肤加载失败'));
        image.src = url;
    });
}

function skinCanvas(image: HTMLImageElement): HTMLCanvasElement {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 64;
    canvas.getContext('2d')?.drawImage(image, 0, 0);
    return canvas;
}

export async function loadFallbackSkin(): Promise<HTMLCanvasElement> {
    return skinCanvas(await loadImage(steveUrl));
}

export async function loadFallbackSkinDataUrl(): Promise<string> {
    return skinCanvas(await loadImage(steveUrl)).toDataURL('image/png');
}
