/* vmux link hints: Rust feeds keys via window.__vmux_hints_feed.
   Fixed-width labels (prefix-free): 1 char ≤26, 2 chars ≤676, 3 chars ≤17576, then 4+.
   While a multi-letter code is still possible, all badges stay bright; only dim non-matches
   when every remaining candidate is a single-letter resolve (vmux-hint-nomatch).
   Only `data-vmux-hints` on <html> marks an active session; its value is the fixed label width (1, 2, …)
   so the shell can continue the session when a DOM read lags after a key.
   `labelStart` / `labelTotal` come from the shell so subframes (e.g. reCAPTCHA) share one global
   hint namespace with the main document. */
(function (labelStart, labelTotal) {
  var letters = "abcdefghijklmnopqrstuvwxyz";

  var prev = window.__vmux_hints_cleanup;
  if (typeof prev === "function") {
    try {
      prev();
    } catch (e) {}
  }

  try {
    document.documentElement.removeAttribute("data-vmux-hint-precount");
  } catch (e) {}

  var sel =
    'a[href],button,input:not([type="hidden"]):not([type="file"]),textarea,select,[role="button"],[role="link"],[role="menuitem"],[role="switch"],[contenteditable=""],[contenteditable="true"],[tabindex]:not([tabindex="-1"]),summary' +
    ",#L2AGLb,#introAgreeButton,button[data-testid],a[data-testid],[data-testid^=\"uc-\"]";

  /** Open shadow roots — keep logic aligned with link_hints_precount.js. */
  function queryHintElementsDeep(root) {
    var acc = [];
    function walk(r) {
      if (!r || !r.querySelectorAll) return;
      var list = r.querySelectorAll(sel);
      for (var i = 0; i < list.length; i++) acc.push(list[i]);
      var star = r.querySelectorAll("*");
      for (var j = 0; j < star.length; j++) {
        var el = star[j];
        if (el.shadowRoot) walk(el.shadowRoot);
      }
    }
    walk(root);
    return acc;
  }

  function viewportSize() {
    var vh =
      typeof innerHeight === "number" && innerHeight > 0
        ? innerHeight
        : document.documentElement && document.documentElement.clientHeight
          ? document.documentElement.clientHeight
          : 0;
    var vw =
      typeof innerWidth === "number" && innerWidth > 0
        ? innerWidth
        : document.documentElement && document.documentElement.clientWidth
          ? document.documentElement.clientWidth
          : 0;
    if (vh < 1) vh = 800;
    if (vw < 1) vw = 1200;
    return { vh: vh, vw: vw };
  }

  function buildNodes(all, strictViewport) {
    var out = [];
    var vs = viewportSize();
    for (var i = 0; i < all.length; i++) {
      var el = all[i];
      if (el.disabled) continue;
      var r = el.getBoundingClientRect();
      if (r.width < 2 || r.height < 2) continue;
      if (strictViewport) {
        if (r.bottom < 0 || r.top > vs.vh || r.right < 0 || r.left > vs.vw) continue;
      }
      var st = window.getComputedStyle(el);
      if (st.visibility === "hidden" || st.display === "none") continue;
      if (st.opacity === "0") continue;
      out.push(el);
    }
    return out;
  }

  var all = queryHintElementsDeep(document.documentElement);
  var nodes = buildNodes(all, true);
  if (nodes.length === 0 && all.length > 0) {
    nodes = buildNodes(all, false);
  }

  var hintHost = document.body || document.documentElement;
  var HINT_BASE = 26;
  var HINT_CAP_1 = HINT_BASE;
  var HINT_CAP_2 = HINT_BASE * HINT_BASE;
  var HINT_CAP_3 = HINT_CAP_2 * HINT_BASE;

  /** Fixed width per page (prefix-free). 1→a..z, 2→aa..zz, 3 after two-letter space is exhausted. */
  function hintWidth(count) {
    if (count <= 0) return 1;
    if (count <= HINT_CAP_1) return 1;
    if (count <= HINT_CAP_2) return 2;
    if (count <= HINT_CAP_3) return 3;
    var w = 4;
    var cap = HINT_CAP_3 * HINT_BASE;
    while (cap < count) {
      w += 1;
      cap *= HINT_BASE;
    }
    return w;
  }

  function hintLabel(i, count) {
    var w = hintWidth(count);
    var n = i;
    var out = "";
    for (var pos = w - 1; pos >= 0; pos--) {
      var pow = Math.pow(HINT_BASE, pos);
      var d = Math.floor(n / pow);
      n %= pow;
      out += letters.charAt(d);
    }
    return out;
  }

  var labels = [];
  for (var li = 0; li < nodes.length; li++) {
    labels.push(hintLabel(labelStart + li, labelTotal));
  }

  try {
    var w = hintWidth(labelTotal);
    document.documentElement.setAttribute("data-vmux-hints", String(w));
  } catch (e) {}

  var style = document.createElement("style");
  style.textContent =
    ".vmux-hint-label{position:fixed;z-index:2147483647;background:#ffeb3b;color:#111;border:2px solid #333;padding:1px 4px;font:bold 11px monospace;line-height:1.2;border-radius:2px;box-shadow:1px 1px 3px rgba(0,0,0,.45);pointer-events:none;opacity:0.95;}";
  style.textContent +=
    ".vmux-hint-muted{color:#777;font-weight:normal;}";
  style.textContent +=
    ".vmux-hint-nomatch{background:#e8dc8f!important;border-color:#888!important;color:#555!important;font-weight:normal!important;opacity:0.72!important;box-shadow:none;}";
  hintHost.appendChild(style);

  var removers = [];
  removers.push(function () {
    if (style.parentNode) style.parentNode.removeChild(style);
  });

  var spans = [];
  for (var j = 0; j < nodes.length; j++) {
    var el = nodes[j];
    var sp = document.createElement("span");
    sp.className = "vmux-hint-label";
    sp.textContent = labels[j];
    var r = el.getBoundingClientRect();
    sp.style.left = Math.max(0, r.left) + "px";
    sp.style.top = Math.max(0, r.top) + "px";
    hintHost.appendChild(sp);
    spans.push(sp);
    removers.push(
      (function (node) {
        return function () {
          if (node.parentNode) node.parentNode.removeChild(node);
        };
      })(sp)
    );
  }

  function doCleanup() {
    try {
      document.documentElement.removeAttribute("data-vmux-hints");
    } catch (e) {}
    try {
      document.documentElement.removeAttribute("data-vmux-hint-precount");
    } catch (e) {}
    for (var k = removers.length - 1; k >= 0; k--) {
      try {
        removers[k]();
      } catch (e) {}
    }
    window.__vmux_hints_feed = null;
    window.__vmux_hints_cleanup = null;
  }

  /** True if some label still extends the current prefix (user may be typing e.g. b → bd). */
  function multicharTargetPossible() {
    if (!typed.length) return false;
    for (var i = 0; i < labels.length; i++) {
      if (labels[i].indexOf(typed) === 0 && labels[i].length > typed.length) return true;
    }
    return false;
  }

  function updateVisible() {
    var dimIrrelevant =
      typed.length > 0 && !multicharTargetPossible();
    for (var i = 0; i < spans.length; i++) {
      spans[i].style.display = "";
      spans[i].classList.remove("vmux-hint-nomatch");
      if (!active[i]) {
        if (dimIrrelevant) {
          spans[i].classList.add("vmux-hint-nomatch");
        }
        while (spans[i].firstChild) spans[i].removeChild(spans[i].firstChild);
        spans[i].textContent = labels[i];
        continue;
      }
      var label = labels[i];
      var prefix = label.slice(0, typed.length);
      var rest = label.slice(typed.length);
      // Avoid `innerHTML`: pages with Trusted Types / strict CSP (e.g. Google)
      // can strip it, leaving empty labels after the first key.
      while (spans[i].firstChild) spans[i].removeChild(spans[i].firstChild);
      if (rest.length === 0) {
        spans[i].textContent = label;
      } else {
        var muted = document.createElement("span");
        muted.className = "vmux-hint-muted";
        muted.textContent = prefix;
        spans[i].appendChild(muted);
        spans[i].appendChild(document.createTextNode(rest));
      }
    }
  }

  function activate(n) {
    doCleanup();
    if (!n || !n.ownerDocument || !n.ownerDocument.contains(n)) return;
    try {
      n.scrollIntoView({ block: "nearest", inline: "nearest" });
    } catch (e) {}
    try {
      if (n.focus) n.focus();
    } catch (e) {}

    var tag = n.tagName && n.tagName.toLowerCase();
    if (tag === "a" && n.href) {
      var attr = (n.getAttribute("href") || "").trim();
      if (
        attr.charAt(0) !== "#" &&
        !/^javascript:/i.test(String(n.href || "")) &&
        n.target !== "_blank"
      ) {
        try {
          window.location.assign(n.href);
          return;
        } catch (eNav) {}
      }
    }

    var r = n.getBoundingClientRect();
    var cx = r.left + Math.max(1, r.width / 2);
    var cy = r.top + Math.max(1, r.height / 2);
    var ev = {
      bubbles: true,
      cancelable: true,
      view: window,
      clientX: cx,
      clientY: cy,
      screenX: cx + (window.screenX || 0),
      screenY: cy + (window.screenY || 0),
      button: 0,
      buttons: 1,
    };
    try {
      n.dispatchEvent(new MouseEvent("mousedown", ev));
      n.dispatchEvent(new MouseEvent("mouseup", ev));
      n.dispatchEvent(new MouseEvent("click", ev));
    } catch (e) {}
    try {
      if (typeof n.click === "function") n.click();
    } catch (e2) {}
  }

  var typed = "";
  var active = [];
  for (var ai = 0; ai < nodes.length; ai++) active.push(true);

  function feedKey(k) {
    if (k.length !== 1 || letters.indexOf(k) < 0) return;
    var next = typed + k;

    var matches = [];
    var exact = -1;
    for (var s = 0; s < nodes.length; s++) {
      if (labels[s].indexOf(next) !== 0) continue;
      matches.push(s);
      if (labels[s] === next) exact = s;
    }

    if (matches.length === 0) {
      // No labels share this prefix (e.g. key auto-repeat stacked "bb" for hint "bd").
      // Do not cleanup: Rust would drop LinkHints while badges still show → stuck input.
      return;
    }

    typed = next;
    for (var t = 0; t < nodes.length; t++) {
      active[t] = labels[t].indexOf(typed) === 0;
    }

    updateVisible();

    // Avoid the "double-letter f" case:
    // - When typed length is 1, require uniqueness before activating.
    // - Once typed length >= 2, activate when either:
    //   1) there is an exact label match, or
    //   2) the typed prefix uniquely identifies a single target.
    if (typed.length === 1) {
      if (matches.length === 1 && labels[matches[0]] === typed) {
        activate(nodes[matches[0]]);
      }
      return;
    }
    if (exact >= 0) activate(nodes[exact]);
    else if (matches.length === 1) activate(nodes[matches[0]]);
  }
  window.__vmux_hints_feed = feedKey;
  window.__vmux_hints_cleanup = doCleanup;
})(__VMUX_LABEL_START__, __VMUX_LABEL_TOTAL__);
