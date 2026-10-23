// 内置默认皮肤（本项目自绘的 64x64 经典模型材质，与后端
// src-tauri/resources/skins/default-skin.png 同源）。账号无自定义皮肤时
// 3D 预览与头像兜底渲染这张图。
import defaultSkinUrl from './assets/default-skin.png';

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
    return skinCanvas(await loadImage(defaultSkinUrl));
}

export async function loadFallbackSkinDataUrl(): Promise<string> {
    return skinCanvas(await loadImage(defaultSkinUrl)).toDataURL('image/png');
}
