/* DUCAT landing page — interactive bits.
 * No frameworks. Vanilla JS. Respects prefers-reduced-motion. */

(function () {
  'use strict';

  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  /* ============================================================
   * theme toggle — Mocha default (brand-locked), Latte on demand,
   * choice remembered. The head inline script applies the stored
   * theme before first paint; this only wires the button.
   * ============================================================ */
  function initTheme() {
    var btn = document.getElementById('theme-toggle');
    if (!btn) return;
    btn.addEventListener('click', function () {
      var latte = document.documentElement.getAttribute('data-theme') === 'latte';
      var next = latte ? 'mocha' : 'latte';
      if (next === 'latte') {
        document.documentElement.setAttribute('data-theme', 'latte');
      } else {
        document.documentElement.removeAttribute('data-theme');
      }
      try { localStorage.setItem('ducat-theme', next); } catch (e) { /* private mode */ }
    });
  }

  /* ============================================================
   * stat count-up animation on view
   * ============================================================ */
  function countUp(el) {
    var target = parseInt(el.dataset.target, 10);
    if (!target || reduceMotion) { el.textContent = el.dataset.target; return; }
    var dur = 1100;
    var start = performance.now();
    function tick(now) {
      var t = Math.min((now - start) / dur, 1);
      var v = Math.round(target * (1 - Math.pow(1 - t, 3)));
      el.textContent = v;
      if (t < 1) requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  }
  function countAllStats() {
    document.querySelectorAll('.stat-chip .num[data-target]').forEach(countUp);
  }
  function initStatsCountUp() {
    if (!('IntersectionObserver' in window) || reduceMotion) {
      countAllStats();
      return;
    }
    var row = document.getElementById('stats-row');
    if (!row) return;
    var o = new IntersectionObserver(function (es) {
      if (es[0].isIntersecting) {
        countAllStats();
        o.disconnect();
      }
    });
    o.observe(row);
  }

  /* ============================================================
   * scroll-triggered reveal
   * ============================================================ */
  function initReveal() {
    if (!('IntersectionObserver' in window) || reduceMotion) {
      document.querySelectorAll('.reveal').forEach(function (el) { el.classList.add('in'); });
      return;
    }
    var obs = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) {
          e.target.classList.add('in');
          obs.unobserve(e.target);
        }
      });
    }, { threshold: 0.12 });
    document.querySelectorAll('.reveal').forEach(function (el) { obs.observe(el); });
  }

  document.addEventListener('DOMContentLoaded', function () {
    initTheme();
    initReveal();
    initStatsCountUp();
  });
})();
