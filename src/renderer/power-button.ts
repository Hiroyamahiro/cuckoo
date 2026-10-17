export class PowerButton {
  private animation: Animation | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  constructor(
    private readonly button: HTMLButtonElement,
    private readonly feedback: HTMLElement,
    choose: (enabled: boolean) => void,
  ) {
    button.addEventListener('click', () => {
      const enabled = button.getAttribute('aria-pressed') !== 'true';
      this.sync(enabled);
      this.showFeedback(enabled);
      choose(enabled);
    });
    window.addEventListener('unload', () => this.cancel());
    window
      .matchMedia('(prefers-reduced-motion: reduce)')
      .addEventListener('change', () => this.cancel());
  }
  sync(enabled: boolean): void {
    // トレイ等で状態が変わった場合、古い操作メッセージを残さない。
    if (this.button.getAttribute('aria-pressed') !== String(enabled))
      this.cancel();
    this.button.setAttribute('aria-pressed', String(enabled));
    const label = enabled
      ? '時報をOFFにする（現在ON）'
      : '時報をONにする（現在OFF）';
    this.button.setAttribute('aria-label', label);
    this.button.title = label;
  }
  private cancel(): void {
    clearTimeout(this.timer);
    this.animation?.cancel();
    this.animation = undefined;
    this.feedback.hidden = true;
  }
  private showFeedback(enabled: boolean): void {
    this.cancel();
    this.feedback.textContent = enabled ? '時報ON' : '時報OFF';
    this.feedback.hidden = false;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      this.timer = setTimeout(() => this.cancel(), 1500);
      return;
    }
    const animation = this.feedback.animate(
      [
        {
          opacity: 0,
          transform: 'translate(-50%, 4px)',
          offset: 0,
          easing: 'ease-out',
        },
        { opacity: 1, transform: 'translate(-50%, 0)', offset: 0.3 / 2.1 },
        {
          opacity: 1,
          transform: 'translate(-50%, 0)',
          offset: 1.8 / 2.1,
          easing: 'ease-in',
        },
        { opacity: 0, transform: 'translate(-50%, -3px)', offset: 1 },
      ],
      { duration: 2100, easing: 'linear' },
    );
    this.animation = animation;
    animation.onfinish = () => {
      if (this.animation === animation) this.cancel();
    };
  }
}
