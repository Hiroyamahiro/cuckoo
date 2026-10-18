import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {ClockStatus,UpdateStatus} from './shared/api';
const updateCallbacks = new Set<(s: UpdateStatus) => void>();
await listen<UpdateStatus>('update-status', e => updateCallbacks.forEach(cb => cb(e.payload)));
import type {Settings} from './shared/settings';
import {SettingsSync} from './settings-sync';
import {PresetClient,type PresetReply} from './preset-client';
import {claimPreviewClick} from './preview-client';
import {SoundClient} from './sound-client';
import type {SoundReply,SoundList} from './shared/api';
const sync = new SettingsSync();
const callbacks=new Set<(s:ClockStatus)=>void>();
await listen<ClockStatus>('status',e=>{
 const before=sync.current;
 const accepted=sync.accept(e.payload);
 if(accepted!==before) callbacks.forEach(cb=>cb(accepted));
});
async function getStatus(): Promise<ClockStatus> {return sync.accept(await invoke<ClockStatus>('get_status'));}
async function updateSettings(patch:Partial<Settings>):Promise<ClockStatus>{
 const before=sync.current ?? await getStatus();
 const {sessionId,revision:baseRevision}=before.settingsSync;
 try{return sync.accept(await invoke<ClockStatus>('update_settings',{sessionId,baseRevision,requestId:crypto.randomUUID(),patch}));}
 catch(error){await getStatus();throw error;}
}
const presets = new PresetClient(getStatus,request=>invoke<PresetReply>('edit_preset',{...request}),status=>sync.accept(status));
const sounds=new SoundClient(getStatus,request=>invoke<SoundReply>('audio_library_operation',{...request}),(sessionId,requestId)=>invoke<SoundReply>('audio_library_receipt',{sessionId,requestId}),status=>sync.accept(status));
window.cuckoo={
 getUpdateStatus:()=>invoke<UpdateStatus>('update_status'),
 updateAction:action=>invoke('update_action',{action}),
 onUpdateStatus:cb=>{updateCallbacks.add(cb);return()=>{updateCallbacks.delete(cb)}},
 soundConfirmationPending:()=>sounds.needsConfirmation,
 listSources:async()=>{const reply=await invoke<SoundList>('list_audio_sources');reply.status=sync.accept(reply.status);return reply;},
 soundOperation:action=>sounds.run(action),
 confirmSoundOperation:()=>sounds.confirm(),
 previewSource:async(target,soundId,repeat,volume)=>{claimPreviewClick();await invoke<number>('preview_audio_source',{target,soundId:soundId??null,repeat:repeat??null,volume:volume??null});await getStatus();},
 getStatus,
 onStatusChanged:cb=>{callbacks.add(cb);return()=>{callbacks.delete(cb)}},
 setEnabled:enabled=>updateSettings({enabled}),
 updateSettings,
 editPreset:action=>presets.edit(action),
 test:async()=>{claimPreviewClick();await invoke<number>('test_sound');await getStatus();},
 openCredit:link=>invoke('open_credit',{link}),
 setAppearance:()=>Promise.resolve(),
 windowAction:action=>invoke('window_action',{action}),
 quitIfTrayUnavailable:()=>invoke('quit')
};
await import('./renderer');
