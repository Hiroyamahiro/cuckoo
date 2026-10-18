export const creditLinks = {
  voicevoxDownload: 'https://voicevox.hiroshiba.jp/',
  otologic: 'https://otologic.jp/',
  license: 'https://creativecommons.org/licenses/by/4.0/',
  voicevox: 'https://voicevox.hiroshiba.jp/term/',
  zundamon: 'https://zunko.jp/con_ongen_kiyaku.html',
  himari: 'https://www.meimeihimari.com/terms-of-use',
} as const;
export type CreditLink = keyof typeof creditLinks | 'notices' | `voicevox-policy:${string}`;
