/* vmux visual mode banner */
(function () {
  var id = "vmux-visual-hint";
  var p = document.getElementById(id);
  if (p) p.remove();
  p = document.createElement("div");
  p.id = id;
  p.style.cssText =
    "position:fixed;top:10px;left:50%;transform:translateX(-50%);z-index:2147483646;" +
    "background:#1a237e;color:#e8eaf6;padding:8px 16px;border-radius:8px;" +
    "font:13px/1.4 -apple-system,BlinkMacSystemFont,sans-serif;box-shadow:0 2px 12px rgba(0,0,0,.35);";
  p.textContent = "Visual — y copy selection · Esc exit";
  document.documentElement.appendChild(p);
  window.__vmux_visual_remove = function () {
    try {
      if (p.parentNode) p.parentNode.removeChild(p);
    } catch (e) {}
    window.__vmux_visual_remove = null;
  };
})();
