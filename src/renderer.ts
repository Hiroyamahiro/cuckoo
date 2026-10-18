import { BulkPopover } from './renderer/bulk-popover';
import { CustomChimes } from './renderer/custom-chimes';
import { RouteVolume } from './renderer/route-volume';
import './custom-chimes.css';
import { WindowControls } from './renderer/window-controls';
import './index.css';
import type { ClockStatus } from './shared/api';
import type { Settings, Theme } from './shared/settings';
import { ClockDial } from './renderer/clock-dial';
import { applyBulkSlots, type BulkAction, type Ring } from './renderer/dial-model';
import { currentTime, nextChimeDisplay } from './renderer/clock-time';
import { AppMenu } from './renderer/app-menu';
import type { CreditLink } from './shared/credits';
import { MyPresets } from './renderer/my-presets';
import { DisplayMode } from './renderer/display-mode';
import { PowerButton } from './renderer/power-button';
import {SoundControls} from './renderer/sound-controls';
import {previewBlocked,previewLabel} from './preview-client';
import './sound-controls.css';
import './sound-select.css';
import './ui-details.css';
import {settingsFeedback} from './ui-feedback';
import {toast} from './renderer/toast';
import {AudioFeedback} from './audio-feedback';
const audioFeedback=new AudioFeedback();
const element = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;
new WindowControls();
const bulkPopover = new BulkPopover();
const current = element<HTMLElement>('current');
const currentDate = element<HTMLElement>('current-date');
const next = element<HTMLElement>('next');
const nextDate = element<HTMLElement>('next-date');
const settingsError = element<HTMLElement>('settings-error');
const audioError = element<HTMLElement>('audio-error');
const inputError = element<HTMLElement>('input-error');
const displayMode = new DisplayMode();
const test = element<HTMLButtonElement>('test');
const fieldset = element<HTMLFieldSetElement>('settings');
const enabled = element<HTMLButtonElement>('enabled');
const powerButton = new PowerButton(
  enabled,
  element('power-feedback'),
  (enabled) => change({ enabled }),
);
const clockMode = element<HTMLButtonElement>('clock-mode');
let requestedMinimum = false;
function chooseMinimum(minimumDisplay: boolean): void {
  requestedMinimum = minimumDisplay;
  clockMode.setAttribute('aria-pressed', String(minimumDisplay));
  change({ minimumDisplay });
}
const masterVolume = new RouteVolume(element('master-volume'), value => change({volume: value}), {id:'volume', outputId:'volume-value', titleId:'volume-title', iconId:'volume-icon', label:'全体の音量', heading:true});
const repeats = [
  ...document.querySelectorAll<HTMLInputElement>('[name="hourly-repeat"]'),
];
const halfRepeats=[...document.querySelectorAll<HTMLInputElement>('[name="half-hourly-repeat"]')];
const presets = [
  ...document.querySelectorAll<HTMLButtonElement>('[data-preset]'),
];
const systemTheme = window.matchMedia('(prefers-color-scheme: dark)');
let selectedTheme: Theme = 'system';
let backgroundIllustration = false;
let initialized = false;
let editable = false;
let testing = false;
let pending = 0;
let requestVersion = 0;
let updates = Promise.resolve();
let soundBusy=false;
let lastStatus:ClockStatus|undefined;
const soundControls=new SoundControls(async <T>(action:()=>Promise<T>):Promise<T>=>{
  soundBusy=true;pending++;requestVersion++;
  if(lastStatus){render(lastStatus);ready();}
  const task=updates.then(action);updates=task.then(()=>{},()=>{});
  try{return await task;}finally{pending--;soundBusy=false;await refresh();}
});
const hourVolume = new RouteVolume(element('hour-volume'), hourVolume => change({hourVolume}));
const halfVolume = new RouteVolume(element('half-volume'), halfHourVolume => change({halfHourVolume}));
const customChimes = new CustomChimes(async edit => {
  if (soundBusy || lastStatus?.soundManagement?.busy) throw new Error('音源操作が完了してから変更してください。');
  requestVersion++; pending++;
  const task = updates.then(async () => {
    const current = await window.cuckoo.getStatus();
    const result = await window.cuckoo.updateSettings(edit(current.settings));
    const revision = result.settingsSync.revision;
    // One write only; confirm the existing persistence worker's receipt.
    const deadline = Date.now() + 7000;
    while (Date.now() < deadline) {
      const status = await window.cuckoo.getStatus();
      if (status.settingsSync.savedRevision !== null && status.settingsSync.savedRevision >= revision) return;
      if (status.saveState === 'error' || !status.settingsSync.canEdit) throw new Error(status.settingsNotice || '設定の保存結果を確認してください。');
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    throw new Error('保存結果を確認できません。設定の状態を確認してから再操作してください。');
  });
  updates = task.catch(() => {});
  try { await task; } finally { pending--; await refresh(); }
});
const myPresets = new MyPresets(async (action) => {
  requestVersion++;
  pending++;
  let selected = '';
  const task = updates.then(async () => {
    const result = await window.cuckoo.editPreset(action);
    selected = result.id;
    myPresets.sync(
      result.status.settings.presets,
      result.status.settings.slots,
    );
    inputError.textContent = '';
  });
  updates = task.catch(() => {});
  try {
    await task;
    return selected;
  } finally {
    pending--;
    if (pending === 0) await refresh();
  }
});
const appMenu = new AppMenu(
  (theme) => {
    selectedTheme = theme;
    applyTheme();
    change({ theme });
  },
  (enabled) => change({ backgroundIllustration: enabled }),
  chooseMinimum,
);
let displayedSlots: readonly boolean[] = Array(48).fill(false);
function renderPresets(slots: readonly boolean[]): void {
  displayedSlots = [...slots];
  myPresets.setSlots(slots);
}
const dial = new ClockDial(
  element('dial'),
  (slots) => change({ slots }),
  renderPresets,
  (minutes, enabled) => customChimes.setMinutes(minutes, enabled),
);
dial.setDisabled(true);
function applyTheme(): void {
  document.documentElement.dataset.forest = String(backgroundIllustration);
  document.documentElement.dataset.theme =
    selectedTheme === 'system'
      ? systemTheme.matches
        ? 'dark'
        : 'light'
      : selectedTheme;
  const appearance = document.documentElement.dataset.theme as 'light' | 'dark';
  void window.cuckoo.setAppearance(appearance).catch((error) => {
    inputError.textContent = `タイトルバーの更新に失敗しました：${String(error)}`;
  });
}
systemTheme.addEventListener('change', applyTheme);
applyTheme();
function render(status: ClockStatus, controls = false): void {
  lastStatus=status;
  editable = status.settingsSync.canEdit&&!status.soundManagement?.busy&&!soundBusy;
  soundControls.update(status);
  customChimes.update(status);
  dial.setCustomChimes(status.settings.customChimes ?? []);
  hourVolume.sync(status.settings.hourVolume ?? 100, editable);
  halfVolume.sync(status.settings.halfHourVolume ?? 100, editable);
  myPresets.setWritable(editable);
  if (pending === 0 || controls)
    requestedMinimum = status.settings.minimumDisplay;
  if (pending === 0 || controls)
    myPresets.sync(status.settings.presets, status.settings.slots);
  appMenu.sync(
    status.settings.theme,
    status.trayAvailable,
    status.settings.backgroundIllustration,
    requestedMinimum,
  );
  if (pending === 0 || controls)
    displayMode.setMode(status.settings.minimumDisplay);
  if (
    pending === 0 &&
    (selectedTheme !== status.settings.theme ||
      backgroundIllustration !== status.settings.backgroundIllustration)
  ) {
    selectedTheme = status.settings.theme;
    backgroundIllustration = status.settings.backgroundIllustration;
    applyTheme();
  }
  if (pending === 0 || controls) powerButton.sync(status.settings.enabled);
  const upcoming = nextChimeDisplay(status.enabled, status.next);
  next.textContent = upcoming.time;
  if (upcoming.dateTime) next.setAttribute('datetime', upcoming.dateTime);
  else next.removeAttribute('datetime');
  next.dataset.empty = String(!status.enabled || status.next === null);
  nextDate.textContent = upcoming.date;
  test.disabled = previewBlocked(status) || testing || pending > 0;
  test.textContent=previewLabel(status,status.settings.audioRouting?.onHour.soundId,'試聴');
  const feedback=settingsFeedback(status);
  settingsError.textContent = feedback.text;
  settingsError.dataset.tone=feedback.tone;
  settingsError.setAttribute('role',feedback.tone==='error'?'alert':'status');
  const playbackFailure=audioFeedback.update(status);
  if(playbackFailure)toast(playbackFailure,'error');
  audioError.textContent=status.audio?.state==='Stuck'
    ? status.audio.message||'音声処理の停止を確認できません。再生操作を停止しています。' : '';
  if (pending === 0 || controls) {
    dial.setSlots(status.settings.slots);
    for (const radio of repeats)
      radio.checked = Number(radio.value) === status.settings.hourlyRepeat;
    for (const radio of halfRepeats)
      radio.checked = Number(radio.value) === (status.settings.halfHourlyRepeat??1);
  }
  if (pending === 0 || controls) {
    masterVolume.sync(status.settings.volume, editable);
    selectedTheme = status.settings.theme;
    backgroundIllustration = status.settings.backgroundIllustration;
    applyTheme();
  }
}
function ready(): void {
  initialized = true;
  enabled.disabled = !editable;
  clockMode.disabled = !editable;
  fieldset.disabled = !editable;
  dial.setDisabled(!editable);
  bulkPopover.setWritable(editable);
  presets.forEach((button) => {
    button.disabled = !editable;
  });
  document.querySelectorAll<HTMLButtonElement>('[data-theme-choice],#background-toggle,#minimum-toggle').forEach(button=>button.disabled=!editable);
}
function change(patch: Partial<Settings>): void {
  if(soundBusy||lastStatus?.soundManagement?.busy){inputError.textContent='音源操作の結果を確認してから設定を変更してください。';return;}
  requestVersion++;
  pending++;
  test.disabled = true;
  updates = updates.then(async () => {
    try {
      await window.cuckoo.updateSettings(patch);
      inputError.textContent = '';
    } catch (error) {
      inputError.textContent = `設定の変更に失敗しました：${String(error)}`;
    } finally {
      pending--;
      if (pending === 0) void refresh();
    }
  });
}
async function refresh(): Promise<void> {
  const moment = currentTime(new Date());
  const minute = moment.dateTime.slice(0, 16);
  if (
    current.dataset.minute !== minute ||
    current.textContent !== moment.text
  ) {
    current.textContent = moment.text;
    current.dataset.minute = minute;
  }
  current.setAttribute('datetime', moment.dateTime);
  if (currentDate.textContent !== moment.date)
    currentDate.textContent = moment.date;
  dial.setAngle(moment.angle);
  const version = requestVersion;
  try {
    const status = await window.cuckoo.getStatus();
    if (pending > 0 || version !== requestVersion) return;
    render(status, !initialized);
    ready();
  } catch (error) {
    inputError.textContent = `状態の取得に失敗しました：${String(error)}`;
  }
}
clockMode.addEventListener('click', () => chooseMinimum(!requestedMinimum));
for (const radio of [...repeats,...halfRepeats])
  radio.addEventListener('change', () => {
    if (radio.checked) change(radio.name==='hourly-repeat'
      ? {hourlyRepeat:Number(radio.value) as 1|2}
      : {halfHourlyRepeat:Number(radio.value) as 1|2});
  });
for (const button of presets)
  button.addEventListener('click', () => {
    const slots = applyBulkSlots(displayedSlots, Number(button.dataset.ring ?? 0) as Ring, button.dataset.preset as BulkAction);
    dial.setSlots(slots);
    change({ slots });
  });
test.addEventListener('click', async () => {
  if (testing) return;
  testing = true;
  test.disabled = true;
  audioError.textContent = '';
  try {
    await updates;
    await window.cuckoo.test();
  } catch (error) {
    toast(`再生に失敗しました：${String(error)}`,'error');
  } finally {
    testing = false;
    await refresh();
  }
});
window.addEventListener('preview-cooldown',()=>{if(lastStatus)render(lastStatus);});
window.cuckoo.onStatusChanged((status) => {
  requestVersion++;
  render(status, !initialized);
  ready();
});
void refresh();
for (const link of document.querySelectorAll<HTMLAnchorElement>(
  '[data-credit]',
)) {
  link.addEventListener('click', (event) => {
    event.preventDefault();
    void window.cuckoo
      .openCredit(link.dataset.credit as CreditLink)
      .catch((error) => {
        inputError.textContent = `リンクを開けませんでした：${String(error)}`;
      });
  });
}
let displayTimer: ReturnType<typeof setInterval> | undefined;
function visibility(): void {
  if(displayTimer) clearInterval(displayTimer);
  displayTimer = undefined;
  if(!document.hidden) { void refresh(); displayTimer=setInterval(() => void refresh(),1000); }
}
document.addEventListener("visibilitychange",visibility);
visibility();
