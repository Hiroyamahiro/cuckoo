import palette from '../icons/tabler/palette.svg?raw';
import photo from '../icons/tabler/photo.svg?raw';
import minimum from '../icons/tabler/arrows-minimize.svg?raw';
import refresh from '../icons/tabler/refresh.svg?raw';
import tray from '../icons/tabler/arrow-bar-to-down.svg?raw';
import copyright from '../icons/tabler/copyright.svg?raw';
import headerSvg from '../../assets/icons/header/cuckoo.svg?raw';
import packageInfo from '../../package.json';
import { UpdateMenu } from '../update-menu';
import type { Theme } from '../shared/settings';
export class AppMenu {
  private readonly trigger =
    document.querySelector<HTMLButtonElement>('#app-menu-button')!;
  private readonly menu = document.querySelector<HTMLElement>('#app-menu')!;
  private readonly credits =
    document.querySelector<HTMLDialogElement>('#credits-dialog')!;
  constructor(
    private readonly chooseTheme: (theme: Theme) => void,
    private readonly chooseBackground: (enabled: boolean) => void,
    private readonly chooseMinimum: (enabled: boolean) => void,
  ) {
    const icons: Record<string,string> = {
      '.menu-caption': palette, '#background-toggle': photo,
      '#minimum-toggle': minimum, '#update-action': refresh, '#store-in-tray': tray,
      '#show-credits': copyright,
    };
    for (const item of this.menu.querySelectorAll<HTMLElement>('button,.menu-caption')) {
      const label = document.createElement('span');
      label.className = 'menu-label';
      label.textContent = item.textContent!.trim();
      if (item.id === 'update-action') label.id = 'update-label';
      item.replaceChildren(label);
      for (const [selector, svg] of Object.entries(icons)) if (item.matches(selector)) {
        const icon = document.createElement('span');
        icon.className = 'menu-icon';
        icon.setAttribute('aria-hidden','true');
        icon.innerHTML = svg;
        icon.querySelector('svg')!.setAttribute('focusable','false');
        item.prepend(icon);
      }
    }
    document.querySelector('#header-icon')!.innerHTML = headerSvg;
    document.querySelector('#credits-icon')!.innerHTML = headerSvg;
    const dot = document.createElement('span');
    dot.id = 'update-dot';
    dot.hidden = true;
    document.querySelector('#header-icon')!.append(dot);
    new UpdateMenu();
    document.querySelector('#credits-version')!.textContent =
      `Version ${packageInfo.version}`;
    document.querySelector('#credits-author')!.textContent =
      '© 2026 ひろやまひろし / Hiroyama Hiroshi';
    this.trigger.addEventListener('click', () =>
      this.menu.hidden ? this.open() : this.close(),
    );
    this.trigger.addEventListener('keydown', (event) => {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        this.open(event.key === 'ArrowUp');
      }
    });
    document.addEventListener('pointerdown', (event) => {
      if (
        !this.menu.hidden &&
        !this.menu.contains(event.target as Node) &&
        !this.trigger.contains(event.target as Node)
      )
        this.close(false);
    });
    document.addEventListener('focusin', (event) => {
      if (
        !this.menu.hidden &&
        !this.menu.contains(event.target as Node) &&
        !this.trigger.contains(event.target as Node)
      )
        this.close(false);
    });
    this.menu.addEventListener('keydown', (event) => {
      const items = this.items();
      let index = items.indexOf(document.activeElement as HTMLButtonElement);
      if (event.key === 'Escape') {
        event.preventDefault();
        this.close();
        return;
      }
      if (event.key === 'Tab') {
        this.close(false);
        return;
      }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      index =
        event.key === 'Home'
          ? 0
          : event.key === 'End'
            ? items.length - 1
            : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) %
              items.length;
      items[index]?.focus();
    });
    for (const button of this.menu.querySelectorAll<HTMLButtonElement>(
      '[data-theme-choice]',
    ))
      button.addEventListener('click', () => {
        this.chooseTheme(button.dataset.themeChoice as Theme);
        this.close();
      });
    document
      .querySelector('#background-toggle')!
      .addEventListener('click', () => {
        this.chooseBackground(
          document
            .querySelector('#background-toggle')!
            .getAttribute('aria-checked') !== 'true',
        );
        this.close();
      });
    document.querySelector('#minimum-toggle')!.addEventListener('click', () => {
      this.chooseMinimum(
        document
          .querySelector('#minimum-toggle')!
          .getAttribute('aria-checked') !== 'true',
      );
      this.close();
    });
    document.querySelector('#store-in-tray')!.addEventListener('click', () => {
      this.close();
      void window.cuckoo.windowAction('close').catch((error) => {
        document.querySelector('#input-error')!.textContent =
          `タスクトレイに格納できませんでした：${String(error)}`;
      });
    });
    document.querySelector('#show-credits')!.addEventListener('click', () => {
      this.close(false);
      this.credits.showModal();
    });
    document
      .querySelector('#close-credits')!
      .addEventListener('click', () => this.credits.close());
    this.credits.addEventListener('close', () =>
      this.trigger.focus({ preventScroll: true }),
    );
    this.credits.addEventListener('click', (event) => {
      const rect = this.credits.getBoundingClientRect();
      if (
        event.target === this.credits &&
        (event.clientX < rect.left ||
          event.clientX > rect.right ||
          event.clientY < rect.top ||
          event.clientY > rect.bottom)
      )
        this.credits.close();
    });
    document.querySelector('#fallback-quit')!.addEventListener('click', () => {
      this.close();
      void window.cuckoo.quitIfTrayUnavailable().catch((error) => {
        document.querySelector('#input-error')!.textContent =
          `終了できませんでした：${String(error)}`;
      });
    });
  }
  private items(): HTMLButtonElement[] {
    return [
      ...this.menu.querySelectorAll<HTMLButtonElement>('button:not([hidden]):not(:disabled)'),
    ];
  }
  private open(last = false): void {
    this.menu.hidden = false;
    this.trigger.setAttribute('aria-expanded', 'true');
    const items = this.items();
    (last ? items.at(-1) : items[0])?.focus();
  }
  private close(restoreFocus = true): void {
    this.menu.hidden = true;
    this.trigger.setAttribute('aria-expanded', 'false');
    if (restoreFocus) this.trigger.focus({ preventScroll: true });
  }
  sync(
    theme: Theme,
    trayAvailable: boolean,
    backgroundIllustration: boolean,
    minimumDisplay: boolean,
  ): void {
    document
      .querySelector('#minimum-toggle')!
      .setAttribute('aria-checked', String(minimumDisplay));
    document
      .querySelector('#background-toggle')!
      .setAttribute('aria-checked', String(backgroundIllustration));
    for (const button of this.menu.querySelectorAll<HTMLButtonElement>(
      '[data-theme-choice]',
    ))
      button.setAttribute(
        'aria-checked',
        String(button.dataset.themeChoice === theme),
      );
    document.querySelector<HTMLButtonElement>('#fallback-quit')!.hidden =
      trayAvailable;
  }
}
