import type {ClockStatus} from './shared/api';
export function settingsFeedback(s:ClockStatus):{text:string;tone:'saving'|'error'|'neutral'}{
 if(s.nativeDisplayError||s.settingsSync.errorCode||s.settingsSync.mode==='blocked'||s.saveState==='error')return{text:s.settingsNotice||'設定の保存状態を確認してください。',tone:'error'};
 return{text:'',tone:'neutral'};
}
export function soundRecovery(s:ClockStatus,listProblem:boolean,localPending:boolean,performing:boolean){
 const confirm=!performing&&(localPending||!!s.soundManagement?.busy||!!s.soundManagement?.operation?.outcome.uncertain);
 return{refresh:listProblem,confirm,visible:listProblem||confirm};
}
