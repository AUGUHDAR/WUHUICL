/**
 * 闪屏窗口桥接 shim：把 Electron preload/splash.ts 的 window.kamuclSplash
 * 合同原样搬到 Tauri（通道名与 Electron 完全一致）。
 */
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

window.kamuclSplash = {
  ready: () => {
    void invoke('ipc_send', { channel: 'boot:splash-ready', args: [] }).catch(() => undefined)
  },
  assembled: () => {
    void invoke('ipc_send', { channel: 'boot:assembled', args: [] }).catch(() => undefined)
  },
  finished: () => {
    void invoke('ipc_send', { channel: 'boot:finished', args: [] }).catch(() => undefined)
  },
  failed: (message: string) => {
    void invoke('ipc_send', { channel: 'boot:splash-failed', args: [message] }).catch(
      () => undefined
    )
  },
  onPointer: (callback: (point: { x: number; y: number }) => void) => {
    let dead = false
    let un: (() => void) | null = null
    void listen('boot:pointer', (e) =>
      callback(e.payload as { x: number; y: number })
    ).then((u) => {
      if (dead) u()
      else un = u
    })
    return () => {
      dead = true
      un?.()
    }
  },
  onState: (callback: (state: unknown) => void) => {
    let dead = false
    let un: (() => void) | null = null
    void listen('boot:state', (e) => callback(e.payload)).then((u) => {
      if (dead) u()
      else un = u
    })
    return () => {
      dead = true
      un?.()
    }
  },
  onReveal: (callback: () => void) => {
    let dead = false
    let un: (() => void) | null = null
    void listen('boot:reveal', () => callback()).then((u) => {
      if (dead) u()
      else un = u
    })
    return () => {
      dead = true
      un?.()
    }
  }
}
