export type Ring = 0 | 1;
export type Preset = 'day' | 'hourly' | 'half-hourly' | 'off';
import { timeAngle } from './time-angle';
export function matchingPreset(slots: readonly boolean[]): Preset | null {
  if (slots.length !== 48) return null;
  return (
    (['day', 'hourly', 'half-hourly', 'off'] as const).find((preset) =>
      presetSlots(preset).every((slot, index) => slot === slots[index]),
    ) ?? null
  );
}
export function slotTooltip(slot: number, enabled: boolean): string {
  return `${String(Math.floor(slot / 2)).padStart(2, '0')}:${slot % 2 === 0 ? '00' : '30'}  ${enabled ? 'ON' : 'OFF'}`;
}
export function presetSlots(preset: Preset): boolean[] {
  return Array.from(
    { length: 48 },
    (_, slot) =>
      preset === 'half-hourly' ||
      (preset === 'hourly' && slot % 2 === 0) ||
      (preset === 'day' && slot % 2 === 0 && slot >= 18 && slot <= 36),
  );
}
export const dialSize = 520;
export function point(hour: number, radius: number): { x: number; y: number } {
  const angle = ((timeAngle(hour * 3600) - 90) / 180) * Math.PI;
  return {
    x: 260 + Math.cos(angle) * radius,
    y: 260 + Math.sin(angle) * radius,
  };
}
export function ringRadius(ring: Ring): number {
  return ring === 0 ? 184 : 234;
}
export function hitHour(x: number, y: number, ring: Ring): number | null {
  const dx = x - 260;
  const dy = y - 260;
  if (Math.abs(Math.hypot(dx, dy) - ringRadius(ring)) > 23) return null;
  const turn = (Math.atan2(dy, dx) + Math.PI / 2 + Math.PI * 2) % (Math.PI * 2);
  return (Math.round((turn / (Math.PI * 2)) * 24 - ring * 0.5) + 24) % 24;
}
export function paintHours(
  slots: readonly boolean[],
  ring: Ring,
  from: number,
  to: number,
  enabled: boolean,
): boolean[] {
  const result = [...slots];
  let distance = (to - from + 24) % 24;
  if (distance > 12) distance -= 24;
  const direction = distance < 0 ? -1 : 1;
  for (let step = 0; step <= Math.abs(distance); step++)
    result[((from + direction * step + 24) % 24) * 2 + ring] = enabled;
  return result;
}
