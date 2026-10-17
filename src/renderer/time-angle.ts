// 00:00を真上とし、24時間で時計回りに一周する角度。
export function timeAngle(seconds: number): number {
  return (seconds / 86400) * 360;
}
