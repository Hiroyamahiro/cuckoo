export const appearances = {
  light: { color: '#fafaf8', symbolColor: '#303734', height: 32 },
  dark: { color: '#1f1f1f', symbolColor: '#e7ece5', height: 32 },
} as const;
export type Appearance = keyof typeof appearances;
export function windowAppearance(appearance: Appearance, forest: boolean) {
  return {
    ...appearances[appearance],
    color: forest
      ? appearance === 'light'
        ? '#ffffff'
        : '#181818'
      : appearances[appearance].color,
  };
}
