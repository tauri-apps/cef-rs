/* Keep selector + visibility rules in sync with link_hints.js (precount only). */
(function () {
  try {
    document.documentElement.removeAttribute("data-vmux-hint-precount");
  } catch (e) {}
  var sel =
    'a[href],button,input:not([type="hidden"]):not([type="file"]),textarea,select,[role="button"],[role="link"],[role="menuitem"],[role="switch"],[contenteditable=""],[contenteditable="true"],[tabindex]:not([tabindex="-1"]),summary' +
    ",#L2AGLb,#introAgreeButton,button[data-testid],a[data-testid],[data-testid^=\"uc-\"]";

  /** Google and similar sites put the search field inside closed shadow roots; pierce open roots. */
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

  function countWithViewport(all, strictViewport) {
    var n = 0;
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
      n++;
    }
    return n;
  }

  var all = queryHintElementsDeep(document.documentElement);
  var n = countWithViewport(all, true);
  /* Interstitials (cookie consent) or OSR: innerHeight can be wrong, or modal fills viewport with tight rects. */
  if (n === 0 && all.length > 0) {
    n = countWithViewport(all, false);
  }
  try {
    document.documentElement.setAttribute("data-vmux-hint-precount", String(n));
  } catch (e) {}
})();
