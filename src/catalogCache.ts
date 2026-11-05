import { getVersionCatalog } from './api';
import { createCatalogSession } from '@shared/catalogSession';
import { registerIdleReleasable } from './idleTrim';
export const catalogSession = createCatalogSession(getVersionCatalog);
// 最小化自整理：整个版本目录快照（1000+ 条）在窗口静默时释放，回到前台按需重拉
registerIdleReleasable({ release: () => catalogSession.reset() });
