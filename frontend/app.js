const BACKEND_BASE = window.localStorage.getItem("SHORTENER_BACKEND_BASE") || "http://localhost:8080";
const API_ENDPOINT = `${BACKEND_BASE}/create`;
const WS_ENDPOINT = `${BACKEND_BASE.replace(/^http/i, "ws")}/ws`;

const shortenForm = document.getElementById("shorten-form");
const longUrlInput = document.getElementById("long-url");
const shortenBtn = document.getElementById("shorten-btn");
const btnText = document.getElementById("btn-text");
const btnSpinner = document.getElementById("btn-spinner");
const resultArea = document.getElementById("result-area");
const shortUrlEl = document.getElementById("short-url");
const copyBtn = document.getElementById("copy-btn");
const formError = document.getElementById("form-error");
const logContainer = document.getElementById("log-container");
const clearLogsBtn = document.getElementById("clear-logs");

const connectionStatus = document.getElementById("connection-status");
const statusText = document.getElementById("status-text");
const streamStatus = document.getElementById("stream-status");

const toast = document.getElementById("toast");
const toastTitle = document.getElementById("toast-title");
const toastMessage = document.getElementById("toast-message");
const toastIcon = document.getElementById("toast-icon");

const latencyStat = document.getElementById("latency-stat");
const cacheHitStat = document.getElementById("cache-hit-stat");
const requestCountStat = document.getElementById("request-count-stat");
const latencyBar = document.getElementById("latency-bar");
const hitBar = document.getElementById("hit-bar");

const analyticsRequests = document.getElementById("analytics-requests");
const analyticsReads = document.getElementById("analytics-reads");
const analyticsWrites = document.getElementById("analytics-writes");
const analyticsHits = document.getElementById("analytics-hits");
const analyticsMisses = document.getElementById("analytics-misses");
const analyticsLatency = document.getElementById("analytics-latency");

const apiNodesList = document.getElementById("api-nodes-list");
const redisNodesList = document.getElementById("redis-nodes-list");
const dbShardsList = document.getElementById("db-shards-list");

const historyContainer = document.getElementById("history-container");
const clearHistoryBtn = document.getElementById("clear-history");

const tabLinks = Array.from(document.querySelectorAll("[data-tab-link]"));
const tabViews = Array.from(document.querySelectorAll(".tab-view"));
const newLinkBtn = document.getElementById("new-link-btn");

let ws;
let reconnectAttempts = 0;
let reconnectTimer;
let latestShortUrl = "";

let requestCount = 0;
let readCount = 0;
let writeCount = 0;
let hitCount = 0;
let missCount = 0;
let latencyTotalMs = 0;
let latencySamples = 0;

const apiNodes = new Set();
const redisNodes = new Set();
const dbShards = new Set();

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#039;");
}

function switchTab(tabName) {
  tabLinks.forEach((link) => {
    link.classList.toggle("active", link.dataset.tabLink === tabName);
  });

  tabViews.forEach((view) => {
    view.classList.toggle("hidden", view.dataset.tab !== tabName);
  });
}

function setLoading(isLoading) {
  shortenBtn.disabled = isLoading;
  btnSpinner.classList.toggle("hidden", !isLoading);
  btnText.textContent = isLoading ? "Shortening..." : "Shorten URL";
}

function showError(message) {
  formError.textContent = message;
  formError.classList.remove("hidden");
}

function clearError() {
  formError.textContent = "";
  formError.classList.add("hidden");
}

function showToast(title, message, type = "success") {
  toastTitle.textContent = title;
  toastMessage.textContent = message;

  if (type === "error") {
    toastIcon.innerHTML = '<span class="material-symbols-outlined">error</span>';
    toastIcon.style.color = "#ff6e84";
  } else {
    toastIcon.innerHTML = '<span class="material-symbols-outlined">check_circle</span>';
    toastIcon.style.color = "#32d5a7";
  }

  toast.classList.remove("hidden");
  clearTimeout(showToast._t);
  showToast._t = setTimeout(() => {
    toast.classList.add("hidden");
  }, 2200);
}

function isValidHttpUrl(value) {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function setConnectionState(connected) {
  connectionStatus.classList.toggle("connected", connected);
  connectionStatus.classList.toggle("disconnected", !connected);
  statusText.textContent = connected ? "Connected" : "Disconnected";
  streamStatus.textContent = connected ? "Connected" : "Disconnected";
  streamStatus.classList.toggle("ok", connected);
}

function appendLogRow(htmlText, muted = false) {
  const row = document.createElement("div");
  row.className = "log-row" + (muted ? " muted" : "");

  const time = new Date().toLocaleTimeString("en-GB", { hour12: false });
  row.innerHTML = `<span class="time">${time}</span><p>${htmlText}</p>`;

  logContainer.appendChild(row);
  logContainer.scrollTop = logContainer.scrollHeight;
}

function appendHistoryRow(text) {
  if (!historyContainer) {
    return;
  }

  const placeholder = historyContainer.querySelector(".history-row.muted");
  if (placeholder) {
    placeholder.remove();
  }

  const row = document.createElement("div");
  row.className = "history-row";
  row.textContent = `${new Date().toLocaleTimeString("en-GB", { hour12: false })}  ${text}`;
  historyContainer.prepend(row);
}

function renderNodeList(target, set) {
  if (!target) {
    return;
  }

  target.innerHTML = "";
  if (set.size === 0) {
    const li = document.createElement("li");
    li.textContent = "No data yet";
    target.appendChild(li);
    return;
  }

  Array.from(set)
    .sort()
    .forEach((name) => {
      const li = document.createElement("li");
      li.textContent = name;
      target.appendChild(li);
    });
}

function updateStats(latencyMs) {
  if (Number.isFinite(latencyMs)) {
    latencyTotalMs += Number(latencyMs);
    latencySamples += 1;
  }

  const hitRate = readCount > 0 ? (hitCount / readCount) * 100 : 0;
  const avgLatency = latencySamples > 0 ? latencyTotalMs / latencySamples : null;

  requestCountStat.textContent = String(requestCount);
  cacheHitStat.textContent = readCount > 0 ? `${hitRate.toFixed(1)} %` : "N/A";
  hitBar.style.width = readCount > 0 ? `${Math.max(4, Math.min(100, hitRate))}%` : "0%";

  analyticsRequests.textContent = String(requestCount);
  analyticsReads.textContent = String(readCount);
  analyticsWrites.textContent = String(writeCount);
  analyticsHits.textContent = String(hitCount);
  analyticsMisses.textContent = String(missCount);

  if (avgLatency !== null) {
    latencyStat.textContent = `${avgLatency.toFixed(1)} ms`;
    analyticsLatency.textContent = `${avgLatency.toFixed(1)} ms`;
    const latencyScore = Math.max(6, Math.min(100, 100 - avgLatency));
    latencyBar.style.width = `${latencyScore}%`;
  } else {
    latencyStat.textContent = "N/A";
    analyticsLatency.textContent = "N/A";
    latencyBar.style.width = "0%";
  }

  renderNodeList(apiNodesList, apiNodes);
  renderNodeList(redisNodesList, redisNodes);
  renderNodeList(dbShardsList, dbShards);
}

function processEventPayload(payload) {
  if (!payload || typeof payload !== "object") {
    return `<span class="tag">[RAW]</span> ${escapeHtml(String(payload))}`;
  }

  const type = typeof payload.type === "string" ? payload.type.toUpperCase() : null;
  const status = typeof payload.status === "string" ? payload.status.toUpperCase() : null;
  const node = typeof payload.node === "string" ? payload.node : null;
  const redis = typeof payload.redis === "string" ? payload.redis : null;
  const db = typeof payload.db === "string" ? payload.db : null;
  const cache = typeof payload.cache === "string" ? payload.cache.toUpperCase() : null;
  const source = typeof payload.source === "string" ? payload.source : null;
  const shortCode = typeof payload.short_code === "string" ? payload.short_code : null;
  const latency = Number.isFinite(payload.latency_ms) ? Number(payload.latency_ms) : null;

  if (type === "READ" || type === "WRITE") {
    requestCount += 1;
  }
  if (type === "READ") {
    readCount += 1;
  }
  if (type === "WRITE") {
    writeCount += 1;
  }
  if (type === "READ" && cache === "HIT") {
    hitCount += 1;
  }
  if (type === "READ" && cache === "MISS") {
    missCount += 1;
  }

  if (node) apiNodes.add(node);
  if (redis) redisNodes.add(redis);
  if (db) dbShards.add(db);

  updateStats(latency);

  const parts = [];
  if (type) parts.push(`type=${escapeHtml(type)}`);
  if (status) parts.push(`status=${escapeHtml(status)}`);
  if (node) parts.push(`api=${escapeHtml(node)}`);
  if (redis) parts.push(`redis=${escapeHtml(redis)}`);
  if (db) parts.push(`db=${escapeHtml(db)}`);
  if (cache) parts.push(`cache=${escapeHtml(cache)}`);
  if (source) parts.push(`source=${escapeHtml(source)}`);
  if (shortCode) parts.push(`code=${escapeHtml(shortCode)}`);
  if (latency !== null) parts.push(`latency=${latency}ms`);

  const summary = parts.join(" ") || escapeHtml(JSON.stringify(payload));
  appendHistoryRow(summary);
  return `<span class="tag">[EVENT]</span> ${summary}`;
}

async function handleShorten(event) {
  event.preventDefault();
  clearError();

  const longUrl = longUrlInput.value.trim();

  if (!longUrl) {
    showError("Please enter a destination URL.");
    return;
  }

  if (!isValidHttpUrl(longUrl)) {
    showError("Invalid URL. Use http:// or https://.");
    return;
  }

  setLoading(true);

  try {
    const response = await fetch(API_ENDPOINT, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ long_url: longUrl })
    });

    if (!response.ok) {
      let errorText = "Server error while shortening URL.";
      try {
        const raw = await response.text();
        if (raw) {
          errorText = raw;
        }
      } catch {
        // keep fallback message
      }
      throw new Error(errorText);
    }

    const data = await response.json();
    if (!data.short_url) {
      throw new Error("Backend returned no short URL.");
    }

    latestShortUrl = data.short_url;
    shortUrlEl.href = data.short_url;
    shortUrlEl.textContent = data.short_url;
    resultArea.classList.remove("hidden");

    showToast("Link Created", "Short URL generated successfully.");
  } catch (error) {
    const rawMessage = error && error.message ? error.message : "Request failed.";
    const message = rawMessage.includes("Failed to fetch")
      ? "Cannot reach backend. Ensure API is running and CORS is enabled."
      : rawMessage;

    showError(message);
    showToast("Failed", message, "error");
  } finally {
    setLoading(false);
  }
}

async function handleCopy() {
  if (!latestShortUrl) {
    return;
  }

  try {
    await navigator.clipboard.writeText(latestShortUrl);
    copyBtn.innerHTML = '<span class="material-symbols-outlined">check</span><span>Copied</span>';
    showToast("Copied", "Short URL copied to clipboard.");

    setTimeout(() => {
      copyBtn.innerHTML = '<span class="material-symbols-outlined">content_copy</span><span>Copy</span>';
    }, 1200);
  } catch {
    showToast("Copy Failed", "Clipboard permission denied.", "error");
  }
}

function connectWebSocket() {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
    return;
  }

  setConnectionState(false);
  appendLogRow('<span class="tag">[SYSTEM]</span> Connecting websocket...', true);

  try {
    ws = new WebSocket(WS_ENDPOINT);
  } catch {
    scheduleReconnect();
    return;
  }

  ws.onopen = () => {
    reconnectAttempts = 0;
    setConnectionState(true);
    appendLogRow('<span class="tag">[SYSTEM]</span> WebSocket connected', true);
  };

  ws.onmessage = (event) => {
    try {
      const payload = JSON.parse(event.data);
      appendLogRow(processEventPayload(payload));
    } catch {
      appendLogRow(`<span class="tag">[RAW]</span> ${escapeHtml(event.data)}`);
    }
  };

  ws.onerror = () => {
    appendLogRow('<span class="tag">[WARN]</span> WebSocket error', true);
  };

  ws.onclose = () => {
    setConnectionState(false);
    appendLogRow('<span class="tag">[SYSTEM]</span> WebSocket disconnected', true);
    scheduleReconnect();
  };
}

function scheduleReconnect() {
  if (reconnectTimer) {
    return;
  }

  reconnectAttempts += 1;
  const delay = Math.min(10000, 1000 * Math.pow(1.6, reconnectAttempts));

  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connectWebSocket();
  }, delay);
}

function clearLogs() {
  logContainer.innerHTML = "";
  appendLogRow('<span class="tag">[SYSTEM]</span> Log stream cleared', true);
}

function clearHistory() {
  historyContainer.innerHTML = '<div class="history-row muted">No events yet.</div>';
}

tabLinks.forEach((link) => {
  link.addEventListener("click", (event) => {
    event.preventDefault();
    switchTab(link.dataset.tabLink);
  });
});

newLinkBtn.addEventListener("click", () => {
  switchTab("dashboard");
  longUrlInput.focus();
});

shortenForm.addEventListener("submit", handleShorten);
copyBtn.addEventListener("click", handleCopy);
clearLogsBtn.addEventListener("click", clearLogs);
clearHistoryBtn.addEventListener("click", clearHistory);

switchTab("dashboard");
setConnectionState(false);
updateStats(null);
connectWebSocket();
