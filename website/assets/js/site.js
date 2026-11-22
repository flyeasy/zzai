/* 字在 zzai — 官网交互 */
(function () {
  "use strict";

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var $ = function (s, el) { return (el || document).querySelector(s); };
  var $$ = function (s, el) { return Array.prototype.slice.call((el || document).querySelectorAll(s)); };

  /* ---------- 顶栏 / 移动菜单 ---------- */
  var topbar = $(".topbar");
  var onScroll = function () {
    if (!topbar) return;
    topbar.classList.toggle("is-scrolled", window.scrollY > 12);
  };
  onScroll();
  window.addEventListener("scroll", onScroll, { passive: true });

  var toggle = $(".nav-toggle");
  var menu = $(".mobile-menu");
  if (toggle && menu) {
    toggle.addEventListener("click", function () {
      var open = menu.classList.toggle("is-open");
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
    });
    $$("a", menu).forEach(function (a) {
      a.addEventListener("click", function () {
        menu.classList.remove("is-open");
        toggle.setAttribute("aria-expanded", "false");
      });
    });
  }

  /* ---------- 滚动出现 ---------- */
  var revealEls = $$("[data-reveal]");
  if ("IntersectionObserver" in window && !reduceMotion) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) {
          e.target.classList.add("is-in");
          io.unobserve(e.target);
        }
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.08 });
    revealEls.forEach(function (el) { io.observe(el); });
  } else {
    revealEls.forEach(function (el) { el.classList.add("is-in"); });
  }

  /* ---------- 数字滚动 ---------- */
  var counters = $$("[data-count]");
  var runCount = function (el) {
    var target = parseFloat(el.getAttribute("data-count"));
    var decimals = parseInt(el.getAttribute("data-decimals") || "0", 10);
    var dur = 1300;
    var t0 = null;
    var suffix = el.getAttribute("data-suffix") || "";
    var tick = function (t) {
      if (!t0) t0 = t;
      var p = Math.min(1, (t - t0) / dur);
      var eased = 1 - Math.pow(1 - p, 3);
      var v = target * eased;
      el.textContent = v.toFixed(decimals) + suffix;
      if (p < 1) requestAnimationFrame(tick);
    };
    if (reduceMotion) {
      el.textContent = target.toFixed(decimals) + suffix;
    } else {
      requestAnimationFrame(tick);
    }
  };
  if (counters.length) {
    if ("IntersectionObserver" in window) {
      var cio = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting) {
            runCount(e.target);
            cio.unobserve(e.target);
          }
        });
      }, { threshold: 0.5 });
      counters.forEach(function (el) {
        el.textContent = "0" + (el.getAttribute("data-suffix") || "");
        cio.observe(el);
      });
    } else {
      counters.forEach(runCount);
    }
  }

  /* ---------- 光标光晕 ---------- */
  if (!reduceMotion && window.matchMedia("(pointer:fine)").matches) {
    var glow = document.createElement("div");
    glow.className = "cursor-glow";
    document.body.appendChild(glow);
    var gx = window.innerWidth / 2, gy = window.innerHeight / 2, cx = gx, cy = gy;
    var glowOn = false;
    window.addEventListener("pointermove", function (e) {
      gx = e.clientX; gy = e.clientY;
      if (!glowOn) { glow.style.opacity = "1"; glowOn = true; }
    }, { passive: true });
    (function loop() {
      cx += (gx - cx) * 0.12;
      cy += (gy - cy) * 0.12;
      glow.style.transform = "translate3d(" + cx + "px," + cy + "px,0)";
      requestAnimationFrame(loop);
    })();
  }

  /* ---------- Bento 悬停光 ---------- */
  $$(".b").forEach(function (card) {
    card.addEventListener("pointermove", function (e) {
      var r = card.getBoundingClientRect();
      card.style.setProperty("--mx", (e.clientX - r.left) + "px");
      card.style.setProperty("--my", (e.clientY - r.top) + "px");
    });
  });

  /* ---------- 复制 SHA ---------- */
  $$("[data-copy]").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var target = document.getElementById(btn.getAttribute("data-copy"));
      if (!target) return;
      var text = target.textContent.trim();
      var done = function () {
        var old = btn.textContent;
        btn.textContent = "已复制";
        setTimeout(function () { btn.textContent = old; }, 1600);
      };
      var fallback = function () {
        var ta = document.createElement("textarea");
        ta.value = text;
        ta.setAttribute("readonly", "");
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.select();
        var ok = false;
        try { ok = document.execCommand("copy"); } catch (e) { ok = false; }
        document.body.removeChild(ta);
        if (ok) done();
      };
      if (navigator.clipboard && window.isSecureContext) {
        navigator.clipboard.writeText(text).then(done).catch(fallback);
      } else {
        fallback();
      }
    });
  });

  /* ---------- 模型体积计算 ---------- */
  var calc = $("[data-model-calc]");
  if (calc) {
    var totalEl = $(".total", calc);
    var update = function () {
      var sum = 0;
      $$("input[type=checkbox]", calc).forEach(function (cb) {
        if (cb.checked) sum += parseFloat(cb.getAttribute("data-mb") || "0");
      });
      totalEl.textContent = sum >= 1024 ? (sum / 1024).toFixed(2) + " GB" : Math.round(sum) + " MB";
    };
    $$("input[type=checkbox]", calc).forEach(function (cb) {
      cb.addEventListener("change", update);
    });
    update();
  }

  /* ---------- 文档目录高亮 ---------- */
  var docsNav = $(".docs-nav");
  if (docsNav) {
    var links = $$("a[href^='#']", docsNav);
    var sections = links.map(function (a) { return document.getElementById(a.getAttribute("href").slice(1)); }).filter(Boolean);
    var setActive = function () {
      var y = window.scrollY + 130;
      var current = sections[0];
      sections.forEach(function (s) { if (s.offsetTop <= y) current = s; });
      links.forEach(function (a) {
        a.classList.toggle("active", a.getAttribute("href") === "#" + (current && current.id));
      });
    };
    window.addEventListener("scroll", setActive, { passive: true });
    setActive();
  }

  /* ---------- 年份 ---------- */
  $$("[data-year]").forEach(function (el) { el.textContent = new Date().getFullYear(); });

  /* ---------- 首页背景画布 ---------- */
  var heroCanvas = document.getElementById("hero-canvas");
  if (heroCanvas && !reduceMotion) {
    var hc = heroCanvas.getContext("2d");
    var hw = 0, hh = 0, dpr = Math.min(window.devicePixelRatio || 1, 2);
    var particles = [];
    var ribbons = [
      { amp: 42, freq: 0.0038, speed: 0.00022, y: 0.62, alpha: 0.30, width: 1.1, hue: 258 },
      { amp: 58, freq: 0.0029, speed: 0.00016, y: 0.72, alpha: 0.20, width: 1.0, hue: 190 },
      { amp: 30, freq: 0.0051, speed: 0.00031, y: 0.52, alpha: 0.16, width: 0.9, hue: 280 }
    ];
    var resize = function () {
      hw = heroCanvas.clientWidth;
      hh = heroCanvas.clientHeight;
      heroCanvas.width = Math.floor(hw * dpr);
      heroCanvas.height = Math.floor(hh * dpr);
      hc.setTransform(dpr, 0, 0, dpr, 0, 0);
      particles = [];
      var n = Math.min(70, Math.floor(hw / 22));
      for (var i = 0; i < n; i++) {
        particles.push({
          x: Math.random() * hw,
          y: Math.random() * hh,
          r: Math.random() * 1.5 + 0.3,
          vx: (Math.random() - 0.5) * 0.12,
          vy: (Math.random() - 0.5) * 0.12,
          a: Math.random() * 0.4 + 0.1
        });
      }
    };
    resize();
    window.addEventListener("resize", resize, { passive: true });

    var visible = true;
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (es) { visible = es[0].isIntersecting; }, { threshold: 0 }).observe(heroCanvas);
    }

    var draw = function (t) {
      requestAnimationFrame(draw);
      if (!visible) return;
      hc.clearRect(0, 0, hw, hh);

      // 粒子
      particles.forEach(function (p) {
        p.x += p.vx; p.y += p.vy;
        if (p.x < 0) p.x = hw; if (p.x > hw) p.x = 0;
        if (p.y < 0) p.y = hh; if (p.y > hh) p.y = 0;
        hc.beginPath();
        hc.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        hc.fillStyle = "rgba(190,182,255," + p.a + ")";
        hc.fill();
      });

      // 光带
      ribbons.forEach(function (rb) {
        hc.beginPath();
        for (var x = 0; x <= hw; x += 8) {
          var y = hh * rb.y + Math.sin(x * rb.freq + t * rb.speed) * rb.amp +
            Math.sin(x * rb.freq * 2.3 + t * rb.speed * 1.7) * (rb.amp * 0.25);
          if (x === 0) hc.moveTo(x, y); else hc.lineTo(x, y);
        }
        var g = hc.createLinearGradient(0, 0, hw, 0);
        g.addColorStop(0, "hsla(" + rb.hue + ",90%,70%,0)");
        g.addColorStop(0.5, "hsla(" + rb.hue + ",90%,72%," + rb.alpha + ")");
        g.addColorStop(1, "hsla(" + rb.hue + ",90%,70%,0)");
        hc.strokeStyle = g;
        hc.lineWidth = rb.width;
        hc.stroke();
      });
    };
    requestAnimationFrame(draw);
  }

  /* ---------- 首页演示舞台 ---------- */
  var stage = document.querySelector("[data-demo]");
  if (stage) {
    var capText = $(".cap-text", stage);
    var capState = $(".cap-state span:last-child", stage);
    var insert = $(".insert", stage);
    var tryBtn = $(".try-btn", stage);
    var waveCanvas = $(".wavebox canvas", stage);
    var phrases = [
      "把明天下午三点的评审挪到会议室 B，顺便把设计稿发我一份。",
      "这段先记一下：下周开始灰度，先放 10% 的流量。",
      "帮我把刚才那段话整理成三条要点，语气正式一点。"
    ];
    var phrase = phrases[0];
    var typing = null, finishedTimer = null, autoTimer = null;
    var state = "idle";

    var setState = function (s) {
      state = s;
      stage.setAttribute("data-state", s);
      if (s === "listening") {
        capState.textContent = "在听 · 本机识别";
      } else if (s === "done") {
        capState.textContent = "已上屏";
      } else {
        capState.textContent = "按住说话";
      }
    };

    var setCap = function (text) {
      capText.innerHTML = "";
      if (!text) {
        var ph = document.createElement("span");
        ph.className = "ph";
        ph.textContent = "按住右 Option（Windows 右 Alt）说一句…";
        capText.appendChild(ph);
        return;
      }
      capText.appendChild(document.createTextNode(text));
      var caret = document.createElement("span");
      caret.className = "caret";
      capText.appendChild(caret);
    };

    var resetDoc = function () {
      insert.textContent = "";
      insert.classList.remove("is-in");
    };

    var finish = function () {
      if (state !== "listening") return;
      setState("done");
      insert.textContent = phrase;
      // 触发一次淡入
      void insert.offsetWidth;
      insert.classList.add("is-in");
      setCap(phrase);
      clearTimeout(finishedTimer);
      finishedTimer = setTimeout(function () {
        // 停在结果上，不自动清空；用户再按一次可以重来
      }, 4000);
    };

    var start = function () {
      if (state === "listening") return;
      clearTimeout(autoTimer);
      clearTimeout(finishedTimer);
      phrase = phrases[Math.floor(Math.random() * phrases.length)];
      resetDoc();
      setState("listening");
      setCap("");
      var i = 0;
      clearInterval(typing);
      typing = setInterval(function () {
        i++;
        setCap(phrase.slice(0, i));
        if (i >= phrase.length) {
          clearInterval(typing);
          typing = null;
          setTimeout(finish, 550);
        }
      }, 62);
    };

    var stop = function () {
      if (state !== "listening") return;
      if (typing) { clearInterval(typing); typing = null; }
      finish();
    };

    setCap("");
    setState("idle");

    if (tryBtn) {
      tryBtn.addEventListener("pointerdown", function (e) {
        e.preventDefault();
        tryBtn.setPointerCapture && tryBtn.setPointerCapture(e.pointerId);
        start();
      });
      tryBtn.addEventListener("pointerup", function (e) { e.preventDefault(); stop(); });
      tryBtn.addEventListener("pointercancel", stop);
      tryBtn.addEventListener("pointerleave", function () { if (state === "listening") stop(); });
      tryBtn.addEventListener("keydown", function (e) {
        if (e.key === " " || e.key === "Enter") { e.preventDefault(); start(); }
      });
      tryBtn.addEventListener("keyup", function (e) {
        if (e.key === " " || e.key === "Enter") { e.preventDefault(); stop(); }
      });
    }

    // 首次进入视口时自动演示一遍
    if (!reduceMotion && "IntersectionObserver" in window) {
      var demoIO = new IntersectionObserver(function (entries) {
        if (entries[0].isIntersecting) {
          autoTimer = setTimeout(start, 900);
          demoIO.disconnect();
        }
      }, { threshold: 0.45 });
      demoIO.observe(stage);
    }

    // 波形
    if (waveCanvas) {
      var wc = waveCanvas.getContext("2d");
      var ww = 0, wh = 0;
      var wresize = function () {
        var r = waveCanvas.getBoundingClientRect();
        ww = r.width; wh = r.height;
        waveCanvas.width = Math.floor(ww * dpr);
        waveCanvas.height = Math.floor(wh * dpr);
        wc.setTransform(dpr, 0, 0, dpr, 0, 0);
      };
      wresize();
      window.addEventListener("resize", wresize, { passive: true });
      var bars = 22;
      var wdraw = function (t) {
        requestAnimationFrame(wdraw);
        wc.clearRect(0, 0, ww, wh);
        var grad = wc.createLinearGradient(0, 0, ww, 0);
        grad.addColorStop(0, "#7c6cff");
        grad.addColorStop(1, "#22d3ee");
        wc.fillStyle = grad;
        var bw = Math.max(1.5, ww / bars - 2.4);
        var gap = 2.4;
        for (var i = 0; i < bars; i++) {
          var base = state === "listening" ? 0.28 : 0.12;
          var amp = state === "listening"
            ? base + Math.abs(Math.sin(t * 0.006 + i * 0.55)) * 0.62 +
              Math.abs(Math.sin(t * 0.013 + i * 1.3)) * 0.24
            : base + Math.abs(Math.sin(t * 0.0016 + i * 0.4)) * 0.08;
          amp = Math.min(1, amp);
          var h = Math.max(3, wh * amp);
          var x = i * (bw + gap);
          var y = (wh - h) / 2;
          if (wc.roundRect) {
            wc.beginPath();
            wc.roundRect(x, y, bw, h, bw / 2);
            wc.fill();
          } else {
            wc.fillRect(x, y, bw, h);
          }
        }
      };
      requestAnimationFrame(wdraw);
    }
  }
})();

/* 下载页签：轻量版 / 完整离线版 */
(function () {
  const bar = document.querySelector(".dl-switch");
  if (!bar) return;
  const btns = Array.from(bar.querySelectorAll("button[data-dl]"));
  const panels = Array.from(document.querySelectorAll("[data-dl-panel]"));
  const apply = (v) => {
    panels.forEach((p) => { p.hidden = p.dataset.dlPanel !== v; });
    btns.forEach((b) => {
      const on = b.dataset.dl === v;
      b.classList.toggle("on", on);
      b.setAttribute("aria-selected", String(on));
    });
  };
  btns.forEach((b) => b.addEventListener("click", () => apply(b.dataset.dl)));
  apply("light");
})();
