import { errText } from '../../../api';
import { toast } from '../../../store';
import type { Settings } from '@shared/types';
import { updateSettings } from '../data';

export function useSettingsPersistence() {
    async function save(
        patch: Partial<Settings>,
        failureMessage = '保存设置失败：'
    ): Promise<boolean> {
        try {
            await updateSettings(patch);
            return true;
        } catch (error) {
            toast(failureMessage + errText(error), 'error');
            return false;
        }
    }
    return { save };
}
