export class DisplayMode {
  private minimum: boolean | undefined;
  private measurement = 0;
  private dimensions = '';
  private fade: Animation | undefined;
  private readonly controls =
    document.querySelector<HTMLElement>('#normal-controls')!;
  private readonly motion = window.matchMedia(
    '(prefers-reduced-motion: reduce)',
  );
  constructor(
    private readonly dial: HTMLElement,
    private readonly report: (error: unknown) => void,
  ) {
    const observer = new ResizeObserver(() => {
      const dimensions = `${Math.round(dial.getBoundingClientRect().width)}:${this.contentTop()}`;
      if (dimensions !== this.dimensions) {
        this.dimensions = dimensions;

        this.fit();
      }
    });
    observer.observe(dial);
    observer.observe(document.querySelector('header')!);
    const refit=()=>this.fit();
    window.addEventListener('native-display-refit',refit);
    this.motion.addEventListener('change', () => {
      this.fade?.cancel();

      this.controls.hidden = this.minimum === true;
    });
    window.addEventListener('unload', () => {
      observer.disconnect();
      window.removeEventListener('native-display-refit',refit);
      this.measurement++;
      this.fade?.cancel();
    });
  }
  private contentTop(): number {
    return Math.ceil(
      document.querySelector('header')!.getBoundingClientRect().bottom,
    );
  }
  setMode(minimum: boolean): void {
    if (minimum === this.minimum) return;
    const initial = this.minimum === undefined;

    this.fade?.cancel();

    if (minimum)
      document.documentElement.style.setProperty(
        '--normal-controls-top',
        `${this.controls.offsetTop}px`,
      );
    this.minimum = minimum;
    this.controls.hidden = false;
    this.controls.inert = minimum;
    document.querySelector<HTMLElement>('header')!.inert = minimum;

    document.documentElement.dataset.minimum = String(minimum);
    const button = document.querySelector('#clock-mode')!;
    button.setAttribute('aria-pressed', String(minimum));
    document.querySelector('#display-mode-hint')!.textContent = minimum
      ? '通常表示に戻す'
      : 'ミニマム表示に切り替える';
    window.scrollTo(0, 0);
    if (initial || this.motion.matches) this.controls.hidden = minimum;
    else {
      const fade = this.controls.animate(
        [{ opacity: minimum ? 1 : 0 }, { opacity: minimum ? 0 : 1 }],
        { duration: minimum ? 150 : 220, fill: 'both' },
      );
      this.fade = fade;
      fade.onfinish = () => {
        if (this.fade !== fade) return;
        this.controls.hidden = minimum;
        fade.cancel();
        this.fade = undefined;
      };
    }
    this.fit();
  }
  private fit(): void {
    const measurement = ++this.measurement;
    if (!this.minimum) return;
    // 非表示・他ウィンドウに隠れた起動ではrAFが停止するため、描画フレームを待たない。
    queueMicrotask(() => {
      if (measurement !== this.measurement || !this.minimum) return;
      // 通常表示と同じ位置を保ち、時計盤の下端から必要な高さを決める。
      const height = Math.ceil(
        this.dial.getBoundingClientRect().bottom + window.scrollY + 70,
      );
      void window.cuckoo.fitMinimumDisplay(height).catch(this.report);
    });
  }
}
