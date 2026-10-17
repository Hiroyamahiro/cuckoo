import type {ClockStatus} from './shared/api';
// Receipt identity prevents stale terminal notifications from replacing a
// newer preview. A new WebView does not replay historical notifications.
export class AudioFeedback {
 private initialized=false;
 private seen=new Set<string>();
 update(s:ClockStatus):string|null{
  const a=s.audio;
  const key=a?`${a.lastOutcome?.id}:${a.lastOutcome?.outcome?.state}:${a.state==='Stuck'?a.active?.id:''}`:'';
  const failure=!!a&&(a.lastOutcome?.outcome?.state==='Failed'||a.state==='Stuck');
  const notify=this.initialized&&failure&&!this.seen.has(key);
  if(failure)this.seen.add(key);
  this.initialized=true;
  return notify?(a?.message||'音声を再生できませんでした。'):null;
 }
}
