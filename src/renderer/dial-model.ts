export type Ring = 0 | 1;
export type BulkAction = 'day' | 'all' | 'off';
import { timeAngle } from './clock-time';
export function slotTooltip(slot: number, enabled: boolean): string {
  return `${String(Math.floor(slot / 2)).padStart(2, '0')}:${slot % 2 === 0 ? '00' : '30'}  ${enabled ? 'ON' : 'OFF'}`;
}
// Existing daytime range: 09:00 through 18:00, inclusive.
export function applyBulkSlots(slots: readonly boolean[], ring: Ring, action: BulkAction): boolean[] {
  return Array.from({length:48},(_,slot)=>action==='off' ? false :
    slot%2!==ring ? slots[slot] : action==='all' || (slot>=18 && slot<=36));
}
export const dialSize = 520;
export const dialCenter = dialSize / 2;
export const customRingDots = 120;
// SVG coordinate radius; 8 larger than the previous dialCenter - 6.
export const customRingRadius = dialCenter + 2;
export function point(hour: number, radius: number): { x: number; y: number } {
  const angle = ((timeAngle(hour * 3600) - 90) / 180) * Math.PI;
  return {
    x: dialCenter + Math.cos(angle) * radius,
    y: dialCenter + Math.sin(angle) * radius,
  };
}
export function ringRadius(ring: Ring): number {
  return ring === 0 ? 184 : 230;
}
export function hitHour(x: number, y: number, ring: Ring): number | null {
  const dx = x - dialCenter;
  const dy = y - dialCenter;
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
