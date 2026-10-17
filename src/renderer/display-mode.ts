export class DisplayMode {
  private minimum: boolean | undefined;
  private fade: Animation | undefined;
  private movement: Animation | undefined;
  private readonly dial = document.querySelector<HTMLElement>('#dial')!;
  private readonly controls =
    document.querySelector<HTMLElement>('#normal-controls')!;
  private readonly motion = window.matchMedia(
    '(prefers-reduced-motion: reduce)',
  );
  constructor() {
    this.motion.addEventListener('change', () => {
      this.fade?.cancel();
      this.movement?.cancel();

      this.controls.hidden = this.minimum === true;
    });
    window.addEventListener('unload', () => {
      this.fade?.cancel();
      this.movement?.cancel();
    });
  }
  setMode(minimum: boolean): void {
    if (minimum === this.minimum) return;
    const initial = this.minimum === undefined;

    const before = !initial && !this.motion.matches ? this.dial.getBoundingClientRect() : undefined;
    this.movement?.cancel();
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
    button.setAttribute('data-tooltip', document.querySelector('#display-mode-hint')!.textContent!);
    window.scrollTo(0, 0);
    if (before) {
      // Preserve the visible bounds across the mode's layout change. This
      // offsets centering in the still-large viewport while Windows resizes.
      const after = this.dial.getBoundingClientRect();
      const dx = before.x + before.width / 2 - after.x - after.width / 2;
      const dy = before.y + before.height / 2 - after.y - after.height / 2;
      const base = minimum ? 'translate(-50%, -50%)' : 'translate(0, 0)';
      const movement = this.dial.animate([
        { transform: base + ' translate(' + dx + 'px,' + dy + 'px) scale(' + before.width / after.width + ')' },
        { transform: base },
      ], { duration: 220, easing: 'cubic-bezier(0.37, 0, 0.63, 1)', fill: 'both' });
      this.movement = movement;
      movement.onfinish = () => {
        if (this.movement !== movement) return;
        movement.cancel();
        this.movement = undefined;
      };
    }
    if (initial || this.motion.matches) this.controls.hidden = minimum;
    else {
      const fade = this.controls.animate(
        [{ opacity: minimum ? 1 : 0 }, { opacity: minimum ? 0 : 1 }],
        { duration: 220, easing: 'ease-in-out', fill: 'both' },
      );
      this.fade = fade;
      fade.onfinish = () => {
        if (this.fade !== fade) return;
        this.controls.hidden = minimum;
        fade.cancel();
        this.fade = undefined;
      };
    }
  }
}
