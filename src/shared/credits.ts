export const creditLinks = {
  otologic: 'https://otologic.jp/',
  license: 'https://creativecommons.org/licenses/by/4.0/',
} as const;
export type CreditLink = keyof typeof creditLinks;
