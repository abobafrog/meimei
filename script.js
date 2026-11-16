(() => {
  document.documentElement.classList.add("js");
  const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  const canvas = document.getElementById("snow");
  const toggle = document.getElementById("snow-toggle");
  const toggleCopy = toggle.querySelector(".toggle-copy");
  let paused = motionQuery.matches;
  const snow = new PixelSnow(canvas, {density: .45, speed: 1, wind: 0, sway: .5, flakeSize: 1, sizeScale: .95, pixelSize: 2, brightness: 1, glow: .7, twinkle: .38, paused});
  window.froklaSnow = snow;
  new SnowVacuum(snow);
  function setPaused(value) {
    paused = value;
    snow.set({paused});
    toggle.setAttribute("aria-pressed", String(paused));
    toggle.setAttribute("aria-label", paused ? "Возобновить снег" : "Приостановить снег");
    toggleCopy.textContent = paused ? "Снег на паузе" : "Снег идёт";
  }
  document.getElementById("snow-density").addEventListener("input", event => {
    const value = Number(event.target.value);
    document.getElementById("snow-value").textContent = value + "%";
    snow.set({density: value / 100});
  });
  document.getElementById("snow-speed").addEventListener("input", event => snow.set({speed: Number(event.target.value)}));
  document.getElementById("snow-size").addEventListener("input", event => snow.set({flakeSize: Math.min(2, Number(event.target.value)), sizeScale: .5 + Number(event.target.value) * .45}));
  document.getElementById("snow-wind").addEventListener("input", event => snow.set({wind: Number(event.target.value) / 2}));
  document.getElementById("snow-brightness").addEventListener("input", event => snow.set({brightness: Number(event.target.value) / 50}));
  document.getElementById("snow-sway").addEventListener("input", event => snow.set({sway: Number(event.target.value) / 2}));
  toggle.addEventListener("click", () => setPaused(!paused));
  motionQuery.addEventListener("change", event => setPaused(event.matches));
  setPaused(paused);

  const portrait = document.getElementById("portrait-frame");
  const canHover = window.matchMedia("(hover: hover) and (pointer: fine)");
  if (canHover.matches && !motionQuery.matches) {
    const visual = portrait.closest(".hero-visual");
    visual.addEventListener("pointermove", (event) => {
      const bounds = portrait.getBoundingClientRect();
      const x = (event.clientX - bounds.left) / bounds.width - 0.5;
      const y = (event.clientY - bounds.top) / bounds.height - 0.5;
      portrait.style.setProperty("--tilt-x", `${(x * 4).toFixed(2)}deg`);
      portrait.style.setProperty("--tilt-y", `${(-y * 4).toFixed(2)}deg`);
    });
    visual.addEventListener("pointerleave", () => {
      portrait.style.setProperty("--tilt-x", "0deg");
      portrait.style.setProperty("--tilt-y", "0deg");
    });
  }

  const revealItems = document.querySelectorAll(".reveal");
  if ("IntersectionObserver" in window && !motionQuery.matches) {
    const observer = new IntersectionObserver((entries, activeObserver) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-visible");
          activeObserver.unobserve(entry.target);
        }
      });
    }, { threshold: 0.14 });
    revealItems.forEach((item) => observer.observe(item));
  } else {
    revealItems.forEach((item) => item.classList.add("is-visible"));
  }

  const nickname = document.getElementById("nickname");
  const message = document.getElementById("message");
  const nicknameCount = document.getElementById("nickname-count");
  const messageCount = document.getElementById("message-count");
  const nicknameError = document.getElementById("nickname-error");
  const messageError = document.getElementById("message-error");
  const form = document.getElementById("note-form");
  const formResult = document.getElementById("form-result");
  const forbidden = /(?:https?:\/\/|ftp:\/\/|www\.|[\p{L}\p{N}._%+-]+@[\p{L}\p{N}.-]+\.[\p{L}]{2,}|(?:^|\s)[\p{L}\p{N}-]+(?:\.[\p{L}\p{N}-]+)+(?:\/[^\s]*)?|!\[[^\]]*\]\([^)]*\)|\[[^\]]+\]\([^)]+\)|<\s*(?:img|picture|source|svg|iframe|video)\b|data:image\/)/iu;

  function updateCounter(input, output) {
    output.innerHTML = `${input.value.length} <i>/</i> ${input.maxLength}`;
  }
  function validateMessage() {
    const value = message.value.trim();
    message.classList.remove("invalid");
    messageError.textContent = "";
    if (!value) messageError.textContent = "Добавь пару слов, чтобы оставить след.";
    else if (forbidden.test(value)) messageError.textContent = "Ссылки и изображения здесь не принимаются.";
    if (messageError.textContent) message.classList.add("invalid");
    return !messageError.textContent;
  }

  nickname.addEventListener("input", () => {
    updateCounter(nickname, nicknameCount);
    nickname.classList.remove("invalid");
    nicknameError.textContent = forbidden.test(nickname.value) ? "Ссылки и изображения нельзя использовать в нике." : "";
    if (nicknameError.textContent) nickname.classList.add("invalid");
    formResult.textContent = "";
  });
  message.addEventListener("input", () => {
    updateCounter(message, messageCount);
    message.classList.remove("invalid");
    messageError.textContent = message.value && forbidden.test(message.value) ? "Ссылки и изображения здесь не принимаются." : "";
    if (messageError.textContent) message.classList.add("invalid");
    formResult.textContent = "";
  });

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    formResult.textContent = "";
    const name = nickname.value.trim();
    nickname.classList.remove("invalid");
    nicknameError.textContent = "";
    if (!name) nicknameError.textContent = "Добавь ник, чтобы я знал, кого благодарить.";
    if (name.length > 24) nicknameError.textContent = "Ник должен быть короче 24 символов.";
    if (name && forbidden.test(name)) nicknameError.textContent = "Ссылки и изображения нельзя использовать в нике.";
    if (nicknameError.textContent) nickname.classList.add("invalid");
    const messageIsValid = validateMessage();
    if (nicknameError.textContent || !messageIsValid) {
      formResult.textContent = "Проверь поля и попробуй ещё раз.";
      return;
    }
    const button=form.querySelector('[type="submit"]'); if(button.disabled)return;
    button.disabled=true;
    try {

      const response=await window.froklaRequest('/api/notes',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({id:form.dataset.requestId || (form.dataset.requestId=Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('')),nickname:name,message:message.value.trim()})});
      if(!response.ok){const error=await response.json();if(response.status===429){formResult.textContent=error.error;return}throw Error()}
      await response.json();delete form.dataset.requestId;form.reset();updateCounter(nickname,nicknameCount);updateCounter(message,messageCount);
      formResult.textContent=window.froklaLocal?'Послание сохранено в этом браузере. Шарик на ёлке!':'Послание отправлено на проверку. Шарик добавлен на ёлку!';window.dispatchEvent(new Event('tree-update'));
    } catch {formResult.textContent=window.froklaLocal?'Не удалось сохранить: хранилище недоступно или заполнено.':'Нет связи с сервером. Послание не отправлено.'}
    finally{button.disabled=false;}
  });
})();
