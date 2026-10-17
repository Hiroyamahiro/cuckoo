export type Theme = 'system' | 'light' | 'dark';
export const defaultSoundId = 'otologic-cuckoo-clock';
export interface SoundSettings {
  soundId: string;
  hourlyRepeat: 1 | 2;
  halfHourlyRepeat: 1 | 2;
  volume: number;
}
export function playbackRepeat(minute: number, hourlyRepeat: 1 | 2, halfHourlyRepeat:1|2=1): 1 | 2 {
  return minute === 0 ? hourlyRepeat : halfHourlyRepeat;
}
export interface Settings extends SoundSettings {
  audioRouting?: {onHour:{soundId:string;repeat:1|2};onHalfHour:{soundId:string;repeat?:1|2}};
  version: 6;
  minimumDisplay: boolean;
  backgroundIllustration: boolean;
  presets: NamedPreset[];
  enabled: boolean;
  // index = 時 × 2 + (00分:0 / 30分:1)。Windowsのローカル時刻を使う。
  slots: boolean[];
  theme: Theme;
}
export interface LegacySchedule {
  minute: 0 | 30;
  start: string;
  end: string;
}
export function inLegacyRange(
  minutes: number,
  start: string,
  end: string,
): boolean {
  const value = (time: string): number =>
    Number(time.slice(0, 2)) * 60 + Number(time.slice(3));
  const first = value(start);
  const last = value(end);
  return (
    first === last ||
    (first < last
      ? minutes >= first && minutes <= last
      : minutes >= first || minutes <= last)
  );
}
export function legacySlots(schedule: LegacySchedule): boolean[] {
  return Array.from(
    { length: 48 },
    (_, slot) =>
      slot % 2 === schedule.minute / 30 &&
      inLegacyRange(slot * 30, schedule.start, schedule.end),
  );
}
export const defaultSettings: Readonly<Settings> = {
  version: 6,
  minimumDisplay: false,
  backgroundIllustration: true,
  presets: [],
  soundId: defaultSoundId,
  hourlyRepeat: 2,
  halfHourlyRepeat: 1,
  enabled: true,
  slots: legacySlots({ minute: 0, start: '09:00', end: '21:00' }),
  volume: 50,
  theme: 'system',
};
export type SaveState = 'initial' | 'saved' | 'pending' | 'error';
export const presetNameLimit = 40;
export const presetLimit = 100;
export interface NamedPreset {
  id: string;
  name: string;
  slots: boolean[];
}
export type PresetAction =
  | { type: 'create'; name: string }
  | { type: 'rename'; id: string; name: string }
  | { type: 'apply' | 'overwrite' | 'delete'; id: string };
