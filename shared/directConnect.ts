import type { InstalledVersion } from './types'

export interface DirectEndpoint { host: string; port: number; kind: 'ipv6' | 'ipv4' | 'lan' }
export interface DirectInvitation {
  format: 'KAMUCL-DIRECT'; version: 1; name: string; minecraftVersion: string
  loader?: string; loaderVersion?: string; endpoints: DirectEndpoint[]; expiresAt: string
}
export interface DirectNetworkInfo {
  addresses: Array<{ name: string; address: string; kind: 'ipv6' | 'ipv4' | 'lan' }>
  gateways: Array<{ address: string; externalAddress: string; diagnosis: string }>
  messages: string[]
}
export interface DirectHostRequest { versionId: string; folder: string; port?: number; useUpnp: boolean; publicAddress?: string }
export interface DirectHostState {
  active: boolean; invite?: string; endpoints: DirectEndpoint[]; messages: string[]
  localPort?: number; exposedPort?: number; connections: number; startedAt?: string
}
export interface DirectOverview { network: DirectNetworkInfo; instances: InstalledVersion[]; state: DirectHostState; detectedPort?: number }
export interface DirectJoinResult { invitation: DirectInvitation; endpoint?: DirectEndpoint; failures: string[] }
