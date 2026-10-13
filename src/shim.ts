/**
 * WUHUICL（Tauri 版）桥接 shim：把 Electron preload 的 window.kamucl 合同
 * 原样搬到 Tauri IPC 上，渲染层 23k 行源码零改动。
 *
 * - invoke/send → Rust 通用路由命令 ipc_invoke / ipc_send（通道名与 Electron 完全一致）
 * - on → Tauri 事件监听（Rust 侧用 window.emit 按通道名推送）
 * - getFilePath → 拖放路径队列：Tauri drag-drop 事件带真实路径，
 *   与 DataTransfer.files 的顺序一一对应，按调用次序出队
 * - 拖放 UX：dragDropEnabled 拦截了 HTML5 拖放事件，这里把 tauri://drag-*
 *   转写回合成 DragEvent（带 File 列表），渲染层的拖入遮罩/分类逻辑照常工作
 */
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen } from '@tauri-apps/api/event'

interface DragPayload {
  paths: string[]
  position: { x: number; y: number }
}

/** 每次 OS 拖入的路径队列（与 drop 事件的 files 顺序对应） */
let dropPaths: string[] = []
let dragActive = false
let lastDragTarget: Element | null = null

function basename(p: string): string {
  const i = Math.max(p.lastIndexOf('\\'), p.lastIndexOf('/'))
  return i >= 0 ? p.slice(i + 1) : p
}

function pointFrom(payload: DragPayload): { x: number; y: number } {
  const dpr = window.devicePixelRatio || 1
  return { x: payload.position.x / dpr, y: payload.position.y / dpr }
}

/** 构造带 File 列表的合成拖放事件并派发到指针下的元素（冒泡到 window 监听器） */
function dispatchDrag(type: string, payload: DragPayload): void {
  const pt = pointFrom(payload)
  const target = document.elementFromPoint(pt.x, pt.y) ?? document.body
  const dt = new DataTransfer()
  if (type === 'drop') {
    for (const p of payload.paths) dt.items.add(new File([], basename(p)))
  } else {
    // enter/over 也要让 e.dataTransfer.types 含 Files，acceptsImportDrag 才放行
    dt.items.add(new File([], 'wuhuicl-drag'))
  }
  const event = new DragEvent(type, {
    bubbles: true,
    cancelable: true,
    composed: true,
    clientX: pt.x,
    clientY: pt.y,
    dataTransfer: dt
  })
  target.dispatchEvent(event)
  lastDragTarget = target
}

function listenTauriDrag(): void {
  listen<DragPayload>('tauri://drag-enter', (e) => {
    dragActive = true
    dispatchDrag('dragenter', e.payload)
  })
  listen<DragPayload>('tauri://drag-over', (e) => {
    if (!dragActive) {
      dragActive = true
      dispatchDrag('dragenter', e.payload)
      return
    }
    const pt = pointFrom(e.payload)
    const target = document.elementFromPoint(pt.x, pt.y) ?? document.body
    if (target !== lastDragTarget) {
      // 目标变了：先在旧目标上补 leave（保持 App.vue 的 dragDepth 计数平衡），再 enter
      lastDragTarget?.dispatchEvent(
        new DragEvent('dragleave', { bubbles: true, cancelable: true })
      )
      dispatchDrag('dragenter', e.payload)
    } else {
      dispatchDrag('dragover', e.payload)
    }
  })
  listen<DragPayload>('tauri://drag-drop', (e) => {
    dropPaths = [...e.payload.paths]
    dispatchDrag('drop', e.payload)
    dragActive = false
    lastDragTarget = null
  })
  listen('tauri://drag-leave', () => {
    if (!dragActive) return
    lastDragTarget?.dispatchEvent(
      new DragEvent('dragleave', { bubbles: true, cancelable: true })
    )
    dragActive = false
    lastDragTarget = null
  })
}

function bridge(): void {
  window.kamucl = {
    invoke: (channel: string, ...args: unknown[]) =>
      // Electron 的 ipcRenderer.invoke 失败时 reject 的是带这层前缀的 Error，渲染层 toast 原样显示；
      // 保持一致才不会让同一条错误在两侧文字不同。
      (invoke('ipc_invoke', { channel, args }) as Promise<unknown>).catch((e: unknown) => {
        const m = e instanceof Error ? e.message : String(e)
        throw new Error(`Error invoking remote method '${channel}': Error: ${m}`)
      }),
    on: (channel: string, cb: (...args: unknown[]) => void) => {
      let dead = false
      let un: (() => void) | null = null
      void listen(channel, (e) => cb(e.payload)).then((u) => {
        if (dead) u()
        else un = u
      })
      return () => {
        dead = true
        un?.()
      }
    },
    send: (channel: string, ...args: unknown[]) => {
      void invoke('ipc_send', { channel, args }).catch(() => undefined)
    },
    getFilePath: (file: File) => dropPaths.shift() ?? file.name,
    platform: 'win32'
  }

  // 顶栏拖拽：原版用 CSS -webkit-app-region:drag（整条顶栏，子元素 no-drag 例外）。
  // Tauri 用 JS 等价实现：左键按在顶栏且不在交互元素上时启动系统拖拽。
  // 个性化编辑模式例外（.shell.edit-mode 时顶栏可点击选中板块，不再是拖拽区）。
  window.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return
    const t = e.target as HTMLElement | null
    if (!t?.closest?.('.topbar')) return
    if (t.closest('button, input, textarea, select, a, .float-menu, .topbar-spacer, [contenteditable]')) return
    if (t.closest('.shell.edit-mode')) return
    void getCurrentWindow().startDragging().catch(() => undefined)
  }, true)

  window.addEventListener('DOMContentLoaded', () => {
    listenTauriDrag()
  })
}

bridge()
