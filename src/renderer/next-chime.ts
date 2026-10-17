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
