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

function characterLabel(character) {
  return character.replaceAll("-", " ").toUpperCase();
}

function createCharacterCard(character) {
  const fragment = byId("characterCardTemplate").content.cloneNode(true);
  const card = fragment.querySelector("[data-character]");
  card.dataset.character = character;
  card.querySelector("[data-character-name]").textContent = characterLabel(character);
  card.querySelector("[data-character-kind]").textContent = character === "patch" ? "CAPTAIN" : "CREWMATE";
  card.classList.toggle("secondary", character !== "patch");
  return card;
}

function renderCharacters(characters) {
  const container = byId("characterCards");
  const names = Object.keys(characters || {}).sort((left, right) => {
    if (left === "patch") return -1;
    if (right === "patch") return 1;
    return left.localeCompare(right);
  });

  container.querySelectorAll("[data-character]").forEach(card => {
    if (!names.includes(card.dataset.character)) card.remove();
  });

  names.forEach(character => {
    let card = container.querySelector(`[data-character="${character}"]`);
    if (!card) card = createCharacterCard(character);
    const status = characters[character];
    card.querySelector('[data-readout="mode"]').textContent = String(status.mode || "unknown").toUpperCase();
    card.querySelector('[data-readout="state"]').textContent = String(status.state || "unknown").toUpperCase();
    card.querySelector('[data-readout="location"]').textContent = String(status.location || "unknown").replaceAll("-", " ").toUpperCase();
    card.querySelectorAll("[data-command]").forEach(button => {
      button.classList.toggle("active", button.dataset.command === "auto" && status.mode === "auto");
    });
    card.querySelectorAll("[data-location]").forEach(button => {
      button.classList.toggle("active", button.dataset.location === status.location);
    });
    container.append(card);
  });
}

function render(data) {
  snapshot = data;
  const online = Boolean(data.service?.active);
  byId("statusCard").classList.toggle("online", online);
  byId("statusCard").classList.toggle("error", !online);
  byId("service").textContent = online ? "WATCHER ONLINE" : "WATCHER OFFLINE";
  byId("updated").textContent = `UPDATED ${new Date().toLocaleTimeString([], {hour:"2-digit",minute:"2-digit"})}`;

  renderCharacters(data.characters);
  document.querySelectorAll("[data-setting]").forEach(button => {
    button.classList.toggle("active", data.settings?.[button.dataset.setting] === button.dataset.value);
  });
  document.querySelectorAll("[data-toggle]").forEach(button => {
    const enabled = Boolean(data.settings?.[button.dataset.toggle]);
    button.setAttribute(button.getAttribute("role") === "switch" ? "aria-checked" : "aria-pressed", String(enabled));
  });
  document.querySelectorAll("[data-character-toggle]").forEach(button => {
    const enabled = (data.settings?.characters || []).includes(button.dataset.characterToggle);
    button.setAttribute("aria-checked", String(enabled));
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

function characterCommand(button, fields) {
  const character = button.closest("[data-character]")?.dataset.character;
  return character && character !== "patch" ? {...fields, character} : fields;
}

document.addEventListener("click", event => {
  const button = event.target.closest("button");
  if (!button) return;
  if (button.dataset.command) {
    run(button, () => post(endpoints.command, characterCommand(button, {command:button.dataset.command})));
  } else if (button.dataset.location) {
    run(button, () => post(endpoints.command, characterCommand(button, {command:"move", location:button.dataset.location})));
  } else if (button.dataset.preview) {
    run(button, () => post(endpoints.command, characterCommand(button, {command:"preview", event:button.dataset.preview})));
  } else if (button.dataset.setting) {
    run(button, () => post(endpoints.settings, {[button.dataset.setting]:button.dataset.value}));
  } else if (button.dataset.toggle) {
    const current = Boolean(snapshot?.settings?.[button.dataset.toggle]);
    run(button, () => post(endpoints.settings, {[button.dataset.toggle]:!current}));
  } else if (button.dataset.characterToggle) {
    const character = button.dataset.characterToggle;
    const current = snapshot?.settings?.characters || [];
    const characters = current.includes(character)
      ? current.filter(name => name !== character)
      : [...current, character];
    run(button, () => post(endpoints.settings, {characters}));
  }
});

refresh();
setInterval(refresh, 2000);
