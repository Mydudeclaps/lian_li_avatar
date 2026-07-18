const byId = id => document.getElementById(id);
const endpoints = {
  status: "/api/patch",
  settings: "/api/patch/settings",
  command: "/api/patch/command"
};
let snapshot = null;

async function request(url, options = {}) {
  const response = await fetch(url, {cache:"no-store", ...options});
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error || `Patch bridge returned ${response.status}`);
  return body;
}

function post(url, body) {
  return request(url, {
    method:"POST",
    headers:{"Content-Type":"application/json"},
    body:JSON.stringify(body)
  });
}

function toast(message) {
  const node = byId("toast");
  node.textContent = message;
  node.classList.add("show");
  clearTimeout(window.patchToastTimer);
  window.patchToastTimer = setTimeout(() => node.classList.remove("show"), 1200);
}

function render(data) {
  snapshot = data;
  const online = Boolean(data.service?.active);
  byId("statusCard").classList.toggle("online", online);
  byId("statusCard").classList.toggle("error", !online);
  byId("service").textContent = online ? "WATCHER ONLINE" : "WATCHER OFFLINE";
  byId("updated").textContent = `UPDATED ${new Date().toLocaleTimeString([], {hour:"2-digit",minute:"2-digit"})}`;
  byId("mode").textContent = String(data.mode || "unknown").toUpperCase();
  byId("state").textContent = String(data.state || "unknown").toUpperCase();
  byId("location").textContent = String(data.location || "unknown").replaceAll("-", " ").toUpperCase();

  document.querySelectorAll("[data-command]").forEach(button => {
    button.classList.toggle("active", button.dataset.command === "auto" && data.mode === "auto");
  });
  document.querySelectorAll("[data-location]").forEach(button => {
    button.classList.toggle("active", button.dataset.location === data.location);
  });
  document.querySelectorAll("[data-setting]").forEach(button => {
    button.classList.toggle("active", data.settings?.[button.dataset.setting] === button.dataset.value);
  });
  document.querySelectorAll("[data-toggle]").forEach(button => {
    const enabled = Boolean(data.settings?.[button.dataset.toggle]);
    button.setAttribute(button.getAttribute("role") === "switch" ? "aria-checked" : "aria-pressed", String(enabled));
  });
}

async function refresh() {
  try {
    render(await request(endpoints.status));
  } catch (error) {
    byId("statusCard").classList.add("error");
    byId("service").textContent = "BRIDGE OFFLINE";
  }
}

async function run(button, operation) {
  button.classList.add("loading");
  try {
    render(await operation());
    toast("ORDER SENT");
  } catch (error) {
    toast(error.message || "ORDER LOST");
  } finally {
    button.classList.remove("loading");
  }
}

document.addEventListener("click", event => {
  const button = event.target.closest("button");
  if (!button) return;
  if (button.dataset.command) {
    run(button, () => post(endpoints.command, {command:button.dataset.command}));
  } else if (button.dataset.location) {
    run(button, () => post(endpoints.command, {command:"move", location:button.dataset.location}));
  } else if (button.dataset.preview) {
    run(button, () => post(endpoints.command, {command:"preview", event:button.dataset.preview}));
  } else if (button.dataset.setting) {
    run(button, () => post(endpoints.settings, {[button.dataset.setting]:button.dataset.value}));
  } else if (button.dataset.toggle) {
    const current = Boolean(snapshot?.settings?.[button.dataset.toggle]);
    run(button, () => post(endpoints.settings, {[button.dataset.toggle]:!current}));
  }
});

refresh();
setInterval(refresh, 2000);
