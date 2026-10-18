import type { CustomChime } from './shared/settings';
export function chimeTime(minute: number): string {
  return `${String(Math.floor(minute / 60)).padStart(2, '0')}:${String(minute % 60).padStart(2, '0')}`;
}
export function parseChimeTime(time: string): number {
  if (!/^([01]\d|2[0-3]):[0-5]\d$/.test(time)) throw new Error('時刻を00:00〜23:59で指定してください。');
  return Number(time.slice(0, 2)) * 60 + Number(time.slice(3));
}
export function putChime(chimes: readonly CustomChime[], chime: CustomChime): CustomChime[] {
  if (chimes.some(c => c.id !== chime.id && c.minute === chime.minute)) throw new Error('同じ時刻の時報は登録できません。');
  return [...chimes.filter(c => c.id !== chime.id), chime].sort((a, b) => a.minute - b.minute);
}
export function customMinutes(chimes: readonly CustomChime[]): Set<number> {
  return new Set(chimes.filter(c => c.enabled).map(c => c.minute));
}
export function toggleCustomMinute(chimes: readonly CustomChime[], minute: number): CustomChime[] {
  return chimes.map(c => c.minute === minute ? {...c, enabled: !c.enabled} : c);
}
export function customMinuteTooltip(chimes: readonly CustomChime[], minute: number): string {
  return chimes.filter(c => c.minute === minute)
    .map(c => `${chimeTime(c.minute)}  ${c.enabled ? 'ON' : 'OFF'}`).join('\n');
}
export function stepTimePart(value: number, step: number, limit: number): number {
  return ((value + step) % limit + limit) % limit;
}

// Select registered times along the shortest arc, including midnight wraparound.
export function customMinutesOnArc(minutes: readonly number[], from: number, to: number): number[] {
  let distance = (to - from + 1440) % 1440;
  if (distance > 720) distance -= 1440;
  return minutes.filter(minute => distance >= 0
    ? (minute - from + 1440) % 1440 <= distance
    : (from - minute + 1440) % 1440 <= -distance);
}
export function setCustomMinutes(chimes: readonly CustomChime[], minutes: readonly number[], enabled: boolean): CustomChime[] {
  const selected = new Set(minutes);
  return chimes.map(chime => selected.has(chime.minute) ? {...chime, enabled} : chime);
}
