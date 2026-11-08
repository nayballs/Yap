// **Notifications while the page is in the background** (Remote Control,
// wave 2): a step that needs you, or an agent that finished, as a
// notification from the page itself — there's no push service here (the
// page is open; the phone's browser has to keep it running), and no
// buttons on it: a tap brings the page back to its card, which is answered
// there, by a tap.
//
// What the phones' browsers allow on Yap's own page (a self-signed
// certificate on the home Wi-Fi), 8 Nov 2026:
// - **Chrome on Android** never shows a notification made by a page:
//   `new Notification()` "always throws a TypeError", notifications come
//   only from a service worker (MDN's browser-compat-data, Chromium's
//   2015 decision), and Chrome won't register one on a page whose
//   certificate it only accepted after a warning. So: not possible; the
//   in-page banner, the title's count and the vibration stay.
// - **Firefox for Android** follows desktop Firefox (`new Notification()`,
//   permission asked from a tap only since Firefox 79): possible while
//   Firefox keeps the page running in the background (a locked phone may
//   stop it, and its connection to the PC with it). Not yet checked on a
//   real phone.
// So it's tried, never assumed: the permission is asked only on a tap
// (the settings sheet's switch), never on load, and a first notification
// ("Notifications are on") shows it works — one that can't be made says
// so in the sheet, in Sound details' plain way.

export const NOTIFY_KEY = 'yap.link.notify';

/** Why it can't, in words (the settings sheet's line). */
export const CANT = {
  api: "This browser can't show notifications from this page. The banner at the top of the page still shows.",
  // Chrome on Android: only a service worker may, and a page on the PC's own certificate can't have one.
  worker:
    "This browser shows notifications only from an installed web app (Chrome on Android does), and a page on your PC's own certificate can't be one. The banner at the top of the page still shows.",
  blocked: 'Notifications are blocked for this page. Allow them in the browser’s site settings (the icon left of the address bar), then turn this on again.',
  insecure: "This page isn't on a secure connection, so the browser won't show its notifications.",
};

/**
 * The notifications. `store`: localStorage (the switch, `yap.link.notify`);
 * `onChange()` redraws the settings sheet.
 */
export function createNotify({ store = globalThis.localStorage, onChange } = {}) {
  const N = { want: false, why: '', tags: new Set() };
  try {
    N.want = JSON.parse(store?.getItem(NOTIFY_KEY) || 'false') === true;
  } catch {
    N.want = false;
  }
  const save = () => {
    try {
      store?.setItem(NOTIFY_KEY, JSON.stringify(N.want));
    } catch {
      /* kept for this visit */
    }
  };
  const api = () => typeof window.Notification === 'function';
  const permission = () => (api() ? window.Notification.permission : 'denied');

  /** Can this page show one now (the switch on, allowed, the API there)? */
  const on = () => N.want && api() && permission() === 'granted';

  /** **The switch turned on** (a tap: the permission's asked here, within it). */
  async function enable() {
    N.why = '';
    if (!api()) {
      N.why = CANT.api;
      N.want = false;
      save();
      onChange?.();
      return false;
    }
    if (window.isSecureContext === false) {
      N.why = CANT.insecure;
      N.want = false;
      save();
      onChange?.();
      return false;
    }
    let p = permission();
    if (p === 'default') {
      try {
        p = await window.Notification.requestPermission();
      } catch {
        p = permission();
      }
    }
    if (p !== 'granted') {
      N.why = CANT.blocked;
      N.want = false;
      save();
      onChange?.();
      return false;
    }
    // The first one shows it works here (Chrome on Android throws).
    try {
      const n = new window.Notification('Notifications are on', {
        body: 'Yap tells you here when an agent needs you or finishes, while this page is in the background.',
        tag: 'yap-link-on',
        icon: 'icon.svg',
        silent: true,
      });
      setTimeout(() => n.close?.(), 4000);
    } catch {
      N.why = CANT.worker;
      N.want = false;
      save();
      onChange?.();
      return false;
    }
    N.want = true;
    save();
    onChange?.();
    return true;
  }

  function disable() {
    N.want = false;
    N.why = '';
    save();
    onChange?.();
  }

  /**
   * **One notification**, while the page isn't in front: `tag` once (a
   * card's), `onClick()` when it's tapped (the page comes back to it).
   */
  function show(title, body, tag, onClick) {
    if (!on() || document.visibilityState === 'visible') return false;
    if (tag && N.tags.has(tag)) return false;
    try {
      const n = new window.Notification(title, { body, tag: tag || undefined, icon: 'icon.svg' });
      if (tag) N.tags.add(tag);
      n.onclick = () => {
        try {
          window.focus();
        } catch {
          /* the browser brings it */
        }
        n.close?.();
        onClick?.();
      };
      return true;
    } catch {
      // It stopped working (the permission taken back): said in the sheet.
      N.why = CANT.worker;
      N.want = false;
      save();
      onChange?.();
      return false;
    }
  }

  /** The sheet's line under the switch. */
  function line() {
    if (N.why) return N.why;
    if (N.want && permission() === 'denied') return CANT.blocked;
    if (on()) return 'On: while this page is open in the background, a step that needs you and a finished agent show as notifications. A locked phone may stop the page, so look in now and then.';
    return 'While this page is in the background: a step that needs you, or an agent that finished. Your browser asks first.';
  }

  return {
    enable,
    disable,
    show,
    line,
    get want() {
      return N.want;
    },
    get on() {
      return on();
    },
    get why() {
      return N.why;
    },
  };
}
