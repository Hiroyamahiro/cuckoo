// 針とデジタル表示を、同じローカル時刻のスナップショットから生成する。
export function currentTime(date: Date): {
  text: string;
  angle: number;
  dateTime: string;
  date: string;
} {
  const seconds =
    date.getHours() * 3600 + date.getMinutes() * 60 + date.getSeconds();
  return {
    text: `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`,
    angle: timeAngle(seconds),
    dateTime: date.toISOString(),
    date: `${date.getMonth() + 1}/${date.getDate()} ${'日月火水木金土'[date.getDay()]}`,
  };
}
import { timeAngle } from './time-angle';
