/* vmux find bar (Vimium-style / find mode) */
(function () {
  var id = "vmux-find-ui";
  var p = document.getElementById(id);
  if (p) p.remove();
  p = document.createElement("div");
  p.id = id;
  p.style.cssText =
    "position:fixed;bottom:10px;left:10px;right:10px;max-width:640px;margin:0 auto;z-index:2147483646;" +
    "background:rgba(28,28,30,.96);color:#f5f5f7;padding:10px 14px;border-radius:8px;" +
    "font:15px/1.35 ui-monospace,SFMono-Regular,monospace;box-shadow:0 4px 24px rgba(0,0,0,.45);" +
    "border:1px solid rgba(255,255,255,.12);";
  p.textContent = "/";
  document.documentElement.appendChild(p);
  window.__vmux_find_ui_set = function (t) {
    p.textContent = "/" + (t || "");
  };
  window.__vmux_find_ui_remove = function () {
    try {
      if (p.parentNode) p.parentNode.removeChild(p);
    } catch (e) {}
    window.__vmux_find_ui_set = null;
    window.__vmux_find_ui_remove = null;
  };
})();
