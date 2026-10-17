import type { Settings, SaveState } from './settings';
export interface ClockStatus {
  nativeDisplayError?:string;
  soundManagement?: {eventSeq:number;busy:boolean;operation:{requestId:string;outcome:SoundOutcome}|null};
  audioRoutingAvailability?: {onHour:{state:string;code?:string};onHalfHour:{state:string;code?:string}};
  settingsSync: {
    sessionId: string;
    revision: number;
    savedRevision: number | null;
    eventSeq: number;
    generation: number;
    catalogRef?: unknown;
    mode: 'loading' | 'persistent' | 'blocked';
    loaded: boolean;
    canEdit: boolean;
    canRestore: boolean;
    errorCode: string;
    presetOperation: {requestId:string;outcome:{state:'pending'|'saved'|'failed';id?:string;revision?:number;generation?:number;code?:string;uncertain?:boolean}} | null;
    backupCandidates: {token: string; generation: number}[];
  };
  enabled: boolean;
  next: number | null;
  audioReady: boolean;
  audio?: {
    preview?:{id:number;soundId:string|null;state:string}|null;
    canPreview?:boolean;
    eventSeq:number;
    state:'Unknown'|'Preparing'|'Playing'|'Ready'|'Interrupted'|'Failed'|'Unavailable'|'Stuck'|'Stopped';
    available:boolean;
    canTest:boolean;
    active:import('../audio-client').AudioReceipt|null;
    lastOutcome:import('../audio-client').AudioReceipt|null;
    message:string;
  };
  message: string;
  settings: Settings;
  saveState: SaveState;
  settingsNotice: string;
  trayAvailable: boolean;
}
export interface Playback {
  id: number;
  volume: number;
  soundId: string;
  repeat: 1 | 2;
}
export interface CuckooApi {
  getUpdateStatus(): Promise<UpdateStatus>;
  updateAction(action: 'check' | 'install' | 'cancel'): Promise<void>;
  onUpdateStatus(callback: (status: UpdateStatus) => void): () => void;
  windowAction(action:'state'|'drag'|'minimize'|'maximize'|'close'):Promise<{maximized:boolean}>;
  soundConfirmationPending():boolean;
  listSources():Promise<SoundList>;
  soundOperation(action:SoundAction):Promise<SoundReply>;
  confirmSoundOperation():Promise<SoundReply>;
  previewSource(target:'hour'|'halfHour'|'one',soundId?:string):Promise<void>;
  getStatus(): Promise<ClockStatus>;
  onStatusChanged(callback: (status: ClockStatus) => void): () => void;
  setEnabled(enabled: boolean): Promise<ClockStatus>;
  updateSettings(patch: Partial<Settings>): Promise<ClockStatus>;
  editPreset(
    action: import('./settings').PresetAction,
  ): Promise<{ status: ClockStatus; id: string }>;
  test(): Promise<void>;
  openCredit(link: import('./credits').CreditLink): Promise<void>;
  setAppearance(appearance: 'light' | 'dark'): Promise<void>;
  quitIfTrayUnavailable(): Promise<void>;
}
export interface UpdateStatus {
  seq: number;
  phase: 'idle' | 'checking' | 'available' | 'downloading' | 'launched';
  version: string | null;
  received: number;
  total: number;
  message: string;
}
export type SoundAction={type:'select';target:'hour'|'halfHour';soundId:string}|{type:'register'}|{type:'rename';id:string;name:string}|{type:'unregister';id:string};
export interface SoundOutcome {state:'pending'|'saved'|'failed'|'rejected'|'cancelled';id?:string;code?:string;uncertain?:boolean}
export interface SoundReply {requestId:string;operation:SoundOutcome;status:ClockStatus}
export interface SoundSource {id:string;name:string;kind:'builtin'|'custom';available:boolean;durationMs?:number;errorCode?:string;pinned:boolean}
export interface SoundList {status:ClockStatus;catalogRef?:unknown;sources:SoundSource[];error?:string}
export interface AudioApi {
  ready(): Promise<void>;
  onPlay(callback: (request: Playback) => void): void;
  onCancel(callback: (id: number) => void): void;
  result(id: number, error?: string): void;
}
