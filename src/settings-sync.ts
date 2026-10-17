import type {ClockStatus} from './shared/api';
// Events and invoke responses can arrive out of order. A WebView starts a new
// cache, but never owns or recreates the Rust settings service.
export class SettingsSync {
  current: ClockStatus | undefined;
  accept(next: ClockStatus): ClockStatus {
    const old = this.current;
    if (old && old.settingsSync.sessionId !== next.settingsSync.sessionId) return old;
    if (old) {
      const settings = next.settingsSync.eventSeq < old.settingsSync.eventSeq ? old : next;
      const audio = (next.audio?.eventSeq ?? -1) > (old.audio?.eventSeq ?? -1) || (settings===next && (next.audio?.eventSeq ?? -1)===(old.audio?.eventSeq ?? -1)) ? next : old;
      const sounds = (next.soundManagement?.eventSeq ?? -1) > (old.soundManagement?.eventSeq ?? -1) || (settings===next && (next.soundManagement?.eventSeq ?? -1)===(old.soundManagement?.eventSeq ?? -1)) ? next : old;
      if(settings===old && audio===old && sounds===old) return old;
      if(settings===next && audio===next && sounds===next){this.current=next;return next;}
      this.current={...settings,audio:audio.audio,audioReady:audio.audioReady,message:audio.message,soundManagement:sounds.soundManagement};
      return this.current;
    }
    this.current = next;
    return next;
  }
}
