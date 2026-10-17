// 00:00を真上とし、24時間で時計回りに一周する角度。
export function timeAngle(seconds: number): number {
  return (seconds / 86400) * 360;
}

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

export function nextChimeDisplay(
  enabled: boolean,
  next: number | null,
  now = new Date(),
): { time: string; date: string; dateTime: string } {
  if (!enabled || next === null)
    return { time: !enabled ? '停止中' : '予定なし', date: '', dateTime: '' };
  const scheduled = new Date(next);
  return {
    time: scheduled.toLocaleTimeString('ja-JP', {
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }),
    date:
      scheduled.toDateString() === now.toDateString()
        ? ''
        : scheduled.toLocaleDateString('ja-JP', {
            month: '2-digit',
            day: '2-digit',
          }) +
          ' ' +
          scheduled.toLocaleDateString('ja-JP', { weekday: 'short' }),
    dateTime: scheduled.toISOString(),
  };
}
