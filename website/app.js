/**
 * Zeo Player Landing Page - Interactive Logic & Playground
 */

document.addEventListener("DOMContentLoaded", () => {
  initClipboardButtons();
  initCommandPalette();
  initTabs();
  initMetadataSandbox();
  initEqualizerWorkbench();
  initHotspotLenses();
});

/* ==========================================================================
   1. 剪贴板快速复制功能
   ========================================================================== */
function initClipboardButtons() {
  document.querySelectorAll("[data-copy]").forEach((btn) => {
    btn.addEventListener("click", async (e) => {
      e.preventDefault();
      const text = btn.getAttribute("data-copy");
      if (!text) return;

      try {
        await navigator.clipboard.writeText(text);
        const originalHtml = btn.innerHTML;
        btn.innerHTML = `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="#10b981" stroke-width="2.5"><polyline points="20 6 9 17 4 12"/></svg>`;
        btn.classList.add("copied");

        setTimeout(() => {
          btn.innerHTML = originalHtml;
          btn.classList.remove("copied");
        }, 1800);
      } catch (err) {
        console.error("复制失败:", err);
      }
    });
  });
}

/* ==========================================================================
   2. 全局快捷命令面板 (Command Palette ⌘K / Ctrl+K)
   ========================================================================== */
const COMMANDS = [
  { id: "goto-mgmt", title: "跳转至：24 字段全维曲库治理大表", cat: "导航", action: () => scrollToSection("#management") },
  { id: "goto-sandbox", title: "跳转至：在线交互体验沙盒", cat: "导航", action: () => scrollToSection("#interactive") },
  { id: "goto-playback", title: "跳转至：发烧级音频引擎 (无缝/无爆音EQ)", cat: "导航", action: () => scrollToSection("#playback") },
  { id: "goto-arch", title: "跳转至：纯 Rust 原生架构解析", cat: "导航", action: () => scrollToSection("#architecture") },
  { id: "goto-compare", title: "跳转至：Zeo vs 传统/Electron 对比矩阵", cat: "导航", action: () => scrollToSection("#comparison") },
  { id: "goto-download", title: "跳转至：客户端下载 (macOS / Windows)", cat: "导航", action: () => scrollToSection("#download") },
  { id: "switch-eq", title: "体验：打开 10 段发烧级均衡器调音台", cat: "功能", action: () => { switchTab("equalizer"); scrollToSection("#interactive"); } },
  { id: "switch-meta", title: "体验：打开元数据检查器与模板重命名", cat: "功能", action: () => { switchTab("inspector"); scrollToSection("#interactive"); } },
  { id: "eq-bass", title: "调音预设：立即激活「重低音 (Bass Boost)」", cat: "EQ 调音", action: () => { switchTab("equalizer"); scrollToSection("#interactive"); applyEqPreset("bass"); } },
  { id: "eq-vocal", title: "调音预设：立即激活「清澈人声 (Vocal)」", cat: "EQ 调音", action: () => { switchTab("equalizer"); scrollToSection("#interactive"); applyEqPreset("vocal"); } },
  { id: "eq-flat", title: "调音预设：重置为「平直响应 (Flat)」", cat: "EQ 调音", action: () => { switchTab("equalizer"); scrollToSection("#interactive"); applyEqPreset("flat"); } },
  { id: "download-latest", title: "外部：前往 GitHub Releases 下载最新稳定版本", cat: "下载", action: () => window.open("https://github.com/subf/zeo-player/releases", "_blank") },
  { id: "open-github", title: "外部：访问 Zeo Player GitHub 仓库", cat: "开源", action: () => window.open("https://github.com/subf/zeo-player", "_blank") }
];

function initCommandPalette() {
  const modal = document.getElementById("cmdModalBackdrop");
  const triggerBtn = document.getElementById("cmdTriggerBtn");
  const closeBtn = document.getElementById("cmdCloseBtn");
  const input = document.getElementById("cmdInput");
  const resultsList = document.getElementById("cmdResultsList");

  if (!modal || !triggerBtn || !input || !resultsList) return;

  function openModal() {
    modal.classList.add("open");
    input.value = "";
    renderCommandResults("");
    setTimeout(() => input.focus(), 50);
  }

  function closeModal() {
    modal.classList.remove("open");
  }

  triggerBtn.addEventListener("click", openModal);
  if (closeBtn) closeBtn.addEventListener("click", closeModal);

  modal.addEventListener("click", (e) => {
    if (e.target === modal) closeModal();
  });

  // 全局 ⌘K / Ctrl+K 快捷键监听
  document.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      if (modal.classList.contains("open")) {
        closeModal();
      } else {
        openModal();
      }
    } else if (e.key === "Escape" && modal.classList.contains("open")) {
      closeModal();
    }
  });

  input.addEventListener("input", (e) => {
    renderCommandResults(e.target.value.trim());
  });

  function renderCommandResults(query) {
    const q = query.toLowerCase();
    const filtered = COMMANDS.filter(cmd => 
      !q || cmd.title.toLowerCase().includes(q) || cmd.cat.toLowerCase().includes(q)
    );

    if (filtered.length === 0) {
      resultsList.innerHTML = `<div style="padding: 24px; text-align: center; color: var(--text-dim); font-size: 0.85rem;">未找到与 "${query}" 相关的命令</div>`;
      return;
    }

    resultsList.innerHTML = filtered.map((cmd, idx) => `
      <div class="cmd-item ${idx === 0 ? 'active' : ''}" data-cmd-id="${cmd.id}">
        <div class="cmd-item-left">
          <span class="cmd-category-tag">${cmd.cat}</span>
          <span>${highlightMatch(cmd.title, query)}</span>
        </div>
        <kbd style="font-size: 0.68rem; font-family: var(--font-mono); color: var(--text-dim)">↵</kbd>
      </div>
    `).join("");

    resultsList.querySelectorAll(".cmd-item").forEach(item => {
      item.addEventListener("click", () => {
        const id = item.getAttribute("data-cmd-id");
        const match = COMMANDS.find(c => c.id === id);
        if (match) {
          match.action();
          closeModal();
        }
      });
    });
  }

  input.addEventListener("keydown", (e) => {
    const items = Array.from(resultsList.querySelectorAll(".cmd-item"));
    const activeIdx = items.findIndex(item => item.classList.contains("active"));

    if (e.key === "ArrowDown") {
      e.preventDefault();
      const nextIdx = (activeIdx + 1) % items.length;
      items.forEach(i => i.classList.remove("active"));
      items[nextIdx].classList.add("active");
      items[nextIdx].scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      const prevIdx = (activeIdx - 1 + items.length) % items.length;
      items.forEach(i => i.classList.remove("active"));
      items[prevIdx].classList.add("active");
      items[prevIdx].scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && activeIdx >= 0) {
      e.preventDefault();
      items[activeIdx].click();
    }
  });
}

function highlightMatch(text, query) {
  if (!query) return text;
  const regex = new RegExp(`(${query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi");
  return text.replace(regex, `<span style="color: var(--emerald-light); font-weight: 700;">$1</span>`);
}

function scrollToSection(selector) {
  const el = document.querySelector(selector);
  if (el) {
    el.scrollIntoView({ behavior: "smooth" });
  }
}

function copyText(text) {
  navigator.clipboard.writeText(text);
}

/* ==========================================================================
   3. 交互标签页切换 (Tabs Switcher)
   ========================================================================== */
function initTabs() {
  document.querySelectorAll(".tab-btn").forEach(btn => {
    btn.addEventListener("click", () => {
      const targetTab = btn.getAttribute("data-tab");
      switchTab(targetTab);
    });
  });
}

function switchTab(tabId) {
  document.querySelectorAll(".tab-btn").forEach(b => {
    b.classList.toggle("active", b.getAttribute("data-tab") === tabId);
  });
  document.querySelectorAll(".sandbox-panel").forEach(p => {
    p.classList.toggle("active", p.id === `panel-${tabId}`);
  });

  if (tabId === "equalizer") {
    setTimeout(drawEqCurve, 50);
  }
}

/* ==========================================================================
   4. 元数据检查器与重命名模拟沙盒 (Metadata & Renamer Sandbox)
   ========================================================================== */
const SAMPLE_TRACKS = [
  {
    id: 1,
    fileName: "track_01_onelastkiss_master.flac",
    title: "One Last Kiss",
    artist: "宇多田光 (Hikaru Utada)",
    album: "One Last Kiss EP",
    track: "01",
    year: "2021",
    format: "FLAC",
    sampleRate: "96,000 Hz",
    bitDepth: "24-bit",
    bitrate: "2,840 kbps",
    channels: "2 (Stereo)",
    fileSize: "78.4 MB"
  },
  {
    id: 2,
    fileName: "zimmer_inception_time_lossless.m4a",
    title: "Time",
    artist: "Hans Zimmer",
    album: "Inception (OST)",
    track: "12",
    year: "2010",
    format: "ALAC",
    sampleRate: "192,000 Hz",
    bitDepth: "24-bit",
    bitrate: "4,610 kbps",
    channels: "2 (Stereo)",
    fileSize: "142.1 MB"
  },
  {
    id: 3,
    fileName: "waltz_for_debby_live.aiff",
    title: "Waltz for Debby",
    artist: "Bill Evans Trio",
    album: "Waltz for Debby",
    track: "02",
    year: "1961",
    format: "AIFF",
    sampleRate: "44,100 Hz",
    bitDepth: "16-bit",
    bitrate: "1,411 kbps",
    channels: "2 (Stereo)",
    fileSize: "54.3 MB"
  },
  {
    id: 4,
    fileName: "daftpunk_ram_giorgio.flac",
    title: "Giorgio by Moroder",
    artist: "Daft Punk",
    album: "Random Access Memories",
    track: "03",
    year: "2013",
    format: "FLAC",
    sampleRate: "88,200 Hz",
    bitDepth: "24-bit",
    bitrate: "2,750 kbps",
    channels: "2 (Stereo)",
    fileSize: "185.6 MB"
  }
];

let currentTrack = SAMPLE_TRACKS[0];

function initMetadataSandbox() {
  const listContainer = document.getElementById("trackListContainer");
  const editTitle = document.getElementById("editTitle");
  const editArtist = document.getElementById("editArtist");
  const editAlbum = document.getElementById("editAlbum");
  const editTrack = document.getElementById("editTrack");
  const editYear = document.getElementById("editYear");
  const renamePattern = document.getElementById("renamePattern");

  if (!listContainer) return;

  function renderTrackList() {
    listContainer.innerHTML = SAMPLE_TRACKS.map(t => `
      <div class="track-item ${t.id === currentTrack.id ? 'active' : ''}" data-id="${t.id}">
        <span class="track-idx">${t.track}</span>
        <div class="track-meta">
          <div class="track-name">${t.title}</div>
          <div class="track-artist">${t.artist}</div>
        </div>
        <span class="track-fmt">${t.format}</span>
      </div>
    `).join("");

    listContainer.querySelectorAll(".track-item").forEach(item => {
      item.addEventListener("click", () => {
        const id = parseInt(item.getAttribute("data-id"));
        const found = SAMPLE_TRACKS.find(t => t.id === id);
        if (found) {
          currentTrack = found;
          renderTrackList();
          fillFormFromTrack(currentTrack);
          updateRenamePreview();
        }
      });
    });
  }

  function fillFormFromTrack(track) {
    // 渲染音频规格 Deck
    const specsDeck = document.getElementById("audioSpecsDeck");
    if (specsDeck) {
      specsDeck.innerHTML = `
        <div class="spec-cell">
          <div class="spec-key">格式 / 采样率</div>
          <div class="spec-val">${track.format} • ${track.sampleRate.split(" ")[0]}</div>
        </div>
        <div class="spec-cell">
          <div class="spec-key">位深 / 比特率</div>
          <div class="spec-val">${track.bitDepth} • ${track.bitrate}</div>
        </div>
        <div class="spec-cell">
          <div class="spec-key">体积 / 声道</div>
          <div class="spec-val">${track.fileSize} • 2ch</div>
        </div>
      `;
    }

    if (editTitle) editTitle.value = track.title;
    if (editArtist) editArtist.value = track.artist;
    if (editAlbum) editAlbum.value = track.album;
    if (editTrack) editTrack.value = track.track;
    if (editYear) editYear.value = track.year;
  }

  function updateRenamePreview() {
    const oldFileName = document.getElementById("oldFileName");
    const newFileName = document.getElementById("newFileName");
    if (!oldFileName || !newFileName) return;

    oldFileName.textContent = currentTrack.fileName;

    const pattern = renamePattern ? renamePattern.value : "{track:02d} - {artist} - {title}";
    const trackNum = parseInt(editTrack ? editTrack.value : currentTrack.track) || 1;
    const trackFormatted = String(trackNum).padStart(2, "0");
    const titleVal = editTitle ? editTitle.value : currentTrack.title;
    const artistVal = editArtist ? editArtist.value : currentTrack.artist;
    const albumVal = editAlbum ? editAlbum.value : currentTrack.album;
    const ext = currentTrack.fileName.split(".").pop();

    let computed = pattern
      .replace(/{track:02d}/g, trackFormatted)
      .replace(/{track}/g, String(trackNum))
      .replace(/{artist}/g, artistVal)
      .replace(/{title}/g, titleVal)
      .replace(/{album}/g, albumVal);

    // 自动过滤非法字符并截断
    computed = computed.replace(/[/\\:*?"<>|]/g, "_").trim();
    if (!computed) computed = "untitled";

    newFileName.textContent = `${computed}.${ext}`;
  }

  // 绑定编辑事件
  [editTitle, editArtist, editAlbum, editTrack, editYear, renamePattern].forEach(input => {
    if (input) input.addEventListener("input", updateRenamePreview);
  });

  // 占位符快捷点击按钮
  document.querySelectorAll(".token-btn").forEach(btn => {
    btn.addEventListener("click", () => {
      const token = btn.getAttribute("data-token");
      if (renamePattern && token) {
        renamePattern.value += ` ${token}`;
        updateRenamePreview();
      }
    });
  });

  renderTrackList();
  fillFormFromTrack(currentTrack);
  updateRenamePreview();
}

/* ==========================================================================
   5. 10 段发烧级 RBJ 双极均衡器调音台 (10-Band EQ Workbench)
   ========================================================================== */
const EQ_FREQUENCIES = ["31 Hz", "62 Hz", "125 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "16 kHz"];

const EQ_PRESETS = {
  flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  bass: [5.5, 4.8, 3.5, 2.0, 0.5, 0, 0, 0, 0, 0],
  vocal: [-2.0, -1.0, 0, 2.0, 4.5, 3.8, 2.5, 1.0, 0, -1.0],
  rock: [4.5, 3.2, -1.0, -1.8, 0, 1.5, 3.0, 4.2, 4.5, 4.8],
  jazz: [3.0, 2.0, 1.0, 1.5, -1.0, -1.0, 0, 1.5, 2.5, 3.0],
  classical: [4.0, 3.0, 2.5, 2.0, -1.5, -1.5, 0, 2.0, 3.2, 3.8]
};

let currentEqGains = [...EQ_PRESETS.flat];

function initEqualizerWorkbench() {
  const rack = document.getElementById("eqFadersRack");
  if (!rack) return;

  rack.innerHTML = EQ_FREQUENCIES.map((freq, idx) => `
    <div class="fader-col">
      <span class="fader-db" id="eqDbVal-${idx}">0.0 dB</span>
      <div class="fader-slider-wrap">
        <input type="range" class="vertical-range" id="eqSlider-${idx}" 
               min="-12" max="12" step="0.5" value="0" orient="vertical">
      </div>
      <span class="fader-freq">${freq}</span>
    </div>
  `).join("");

  EQ_FREQUENCIES.forEach((_, idx) => {
    const slider = document.getElementById(`eqSlider-${idx}`);
    if (slider) {
      slider.addEventListener("input", (e) => {
        currentEqGains[idx] = parseFloat(e.target.value);
        updateFaderLabel(idx, currentEqGains[idx]);
        updateHeadroomMeter();
        drawEqCurve();
        clearActivePresetButtons();
      });
    }
  });

  // 绑定预设按钮
  document.querySelectorAll(".preset-btn").forEach(btn => {
    btn.addEventListener("click", () => {
      const presetName = btn.getAttribute("data-preset");
      applyEqPreset(presetName);
    });
  });

  window.addEventListener("resize", drawEqCurve);
  drawEqCurve();
}

function applyEqPreset(name) {
  const values = EQ_PRESETS[name];
  if (!values) return;

  currentEqGains = [...values];
  currentEqGains.forEach((val, idx) => {
    const slider = document.getElementById(`eqSlider-${idx}`);
    if (slider) slider.value = val;
    updateFaderLabel(idx, val);
  });

  document.querySelectorAll(".preset-btn").forEach(b => {
    b.classList.toggle("active", b.getAttribute("data-preset") === name);
  });

  updateHeadroomMeter();
  drawEqCurve();
}

function updateFaderLabel(idx, val) {
  const label = document.getElementById(`eqDbVal-${idx}`);
  if (label) {
    const sign = val > 0 ? "+" : "";
    label.textContent = `${sign}${val.toFixed(1)} dB`;
  }
}

function clearActivePresetButtons() {
  document.querySelectorAll(".preset-btn").forEach(b => b.classList.remove("active"));
}

function updateHeadroomMeter() {
  const maxGain = Math.max(0, ...currentEqGains);
  const headroomDb = (-maxGain * 0.4).toFixed(1);
  const meterBar = document.getElementById("headroomBar");
  const meterVal = document.getElementById("headroomDb");

  if (meterBar) {
    const widthPct = Math.min(100, Math.max(10, (maxGain / 12) * 80 + 15));
    meterBar.style.width = `${widthPct}%`;
  }
  if (meterVal) {
    meterVal.textContent = `${headroomDb} dB`;
  }
}

/**
 * 在 Canvas 上绘制平滑双极频率响应曲线
 */
function drawEqCurve() {
  const canvas = document.getElementById("eqCurveCanvas");
  if (!canvas) return;

  const rect = canvas.getBoundingClientRect();
  canvas.width = rect.width * (window.devicePixelRatio || 1);
  canvas.height = rect.height * (window.devicePixelRatio || 1);

  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const width = canvas.width;
  const height = canvas.height;
  const midY = height / 2;

  ctx.clearRect(0, 0, width, height);

  // 绘制 0dB 基准中线与虚线网格
  ctx.strokeStyle = "rgba(255, 255, 255, 0.08)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(0, midY);
  ctx.lineTo(width, midY);
  ctx.stroke();

  // 绘制 +6dB 与 -6dB 辅助线
  const quarterY = height / 4;
  ctx.beginPath();
  ctx.setLineDash([4, 4]);
  ctx.moveTo(0, quarterY);
  ctx.lineTo(width, quarterY);
  ctx.moveTo(0, height - quarterY);
  ctx.lineTo(width, height - quarterY);
  ctx.stroke();
  ctx.setLineDash([]);

  // 计算 10 个频率点坐标
  const paddingX = width * 0.05;
  const usableWidth = width - paddingX * 2;
  const stepX = usableWidth / 9;

  const points = currentEqGains.map((gain, i) => {
    const x = paddingX + i * stepX;
    // +12dB -> top, -12dB -> bottom
    const y = midY - (gain / 12) * (midY - 10);
    return { x, y };
  });

  // 平滑样条曲线路径
  ctx.beginPath();
  ctx.moveTo(0, points[0].y);
  ctx.lineTo(points[0].x, points[0].y);

  for (let i = 0; i < points.length - 1; i++) {
    const curr = points[i];
    const next = points[i + 1];
    const cpx = (curr.x + next.x) / 2;
    ctx.bezierCurveTo(cpx, curr.y, cpx, next.y, next.x, next.y);
  }

  ctx.lineTo(width, points[points.length - 1].y);

  // 曲线下方的翡翠绿色渐变填充
  ctx.save();
  const fillPath = new Path2D();
  fillPath.moveTo(0, midY);
  fillPath.lineTo(0, points[0].y);
  fillPath.lineTo(points[0].x, points[0].y);
  for (let i = 0; i < points.length - 1; i++) {
    const curr = points[i];
    const next = points[i + 1];
    const cpx = (curr.x + next.x) / 2;
    fillPath.bezierCurveTo(cpx, curr.y, cpx, next.y, next.x, next.y);
  }
  fillPath.lineTo(width, points[points.length - 1].y);
  fillPath.lineTo(width, midY);
  fillPath.closePath();

  const grad = ctx.createLinearGradient(0, 0, 0, height);
  grad.addColorStop(0, "rgba(16, 185, 129, 0.28)");
  grad.addColorStop(0.5, "rgba(16, 185, 129, 0.08)");
  grad.addColorStop(1, "rgba(16, 185, 129, 0.28)");
  ctx.fillStyle = grad;
  ctx.fill(fillPath);
  ctx.restore();

  // 绘制发光的响应曲线主体
  ctx.strokeStyle = "#10b981";
  ctx.lineWidth = 2.5 * (window.devicePixelRatio || 1);
  ctx.shadowColor = "rgba(16, 185, 129, 0.6)";
  ctx.shadowBlur = 8;
  ctx.stroke();

  // 绘制 10 个频点锚点圆圈
  ctx.shadowBlur = 0;
  points.forEach(p => {
    ctx.fillStyle = "#ffffff";
    ctx.beginPath();
    ctx.arc(p.x, p.y, 3.5 * (window.devicePixelRatio || 1), 0, Math.PI * 2);
    ctx.fill();

    ctx.strokeStyle = "#10b981";
    ctx.lineWidth = 1.5;
    ctx.stroke();
  });
}

/* ==========================================================================
   6. 英雄区工作台视窗热点透镜 (Hotspot Lenses)
   ========================================================================== */
function initHotspotLenses() {
  document.querySelectorAll(".workbench-lens").forEach(lens => {
    lens.addEventListener("click", () => {
      const type = lens.getAttribute("data-lens");
      if (type === "table") {
        scrollToSection("#management");
      } else if (type === "inspector") {
        switchTab("inspector");
        scrollToSection("#interactive");
      } else if (type === "toolbar") {
        scrollToSection("#management");
      }
    });
  });
}
