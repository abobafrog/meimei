(() => {
  document.documentElement.classList.add("js");
  const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
  const canvas = document.getElementById("snow");
  const toggle = document.getElementById("snow-toggle");
  const toggleCopy = toggle.querySelector(".toggle-copy");
  const context = canvas.getContext("2d", { alpha: true });
  let flakes = [];
  let frame = 0;
  let paused = motionQuery.matches;
  let width = 0;
  let height = 0;
  let pixelRatio = 1;

  function makeFlakes() {
    const amount = Math.max(18, Math.min(72, Math.round(width / 17)));
    flakes = Array.from({ length: amount }, () => ({
      x: Math.random() * width,
      y: Math.random() * height,
      radius: Math.random() * 1.65 + 0.45,
      speed: Math.random() * 0.34 + 0.12,
      drift: Math.random() * 0.32 + 0.08,
      phase: Math.random() * Math.PI * 2,
      alpha: Math.random() * 0.42 + 0.16,
    }));
  }

  function resize() {
    if (!context) return;
    width = window.innerWidth;
    height = window.innerHeight;
    pixelRatio = Math.min(window.devicePixelRatio || 1, 1.5);
    canvas.width = Math.round(width * pixelRatio);
    canvas.height = Math.round(height * pixelRatio);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    makeFlakes();
    draw(0, false);
  }

  function draw(time, move = true) {
    if (!context) return;
    context.clearRect(0, 0, width, height);
    for (const flake of flakes) {
      if (move) {
        flake.y += flake.speed;
        flake.x += Math.sin(time / 1900 + flake.phase) * flake.drift;
        if (flake.y > height + 5) { flake.y = -5; flake.x = Math.random() * width; }
        if (flake.x < -5) flake.x = width + 5;
        if (flake.x > width + 5) flake.x = -5;
      }
      context.beginPath();
      context.fillStyle = `rgba(210, 232, 250, ${flake.alpha})`;
      context.arc(flake.x, flake.y, flake.radius, 0, Math.PI * 2);
      context.fill();
    }
  }

  function animate(time) {
    if (paused) return;
    draw(time);
    frame = window.requestAnimationFrame(animate);
  }

  function setPaused(value) {
    paused = value;
    window.cancelAnimationFrame(frame);
    toggle.setAttribute("aria-pressed", String(paused));
    toggle.setAttribute("aria-label", paused ? "Возобновить снег" : "Приостановить снег");
    toggleCopy.textContent = paused ? "Снег на паузе" : "Снег идёт";
    if (!paused) frame = window.requestAnimationFrame(animate);
    else draw(0, false);
  }

  toggle.addEventListener("click", () => setPaused(!paused));
  motionQuery.addEventListener("change", (event) => setPaused(event.matches));
  window.addEventListener("resize", resize, { passive: true });
  resize();
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

  form.addEventListener("submit", (event) => {
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
    formResult.textContent = "Проверка пройдена. Записка не отправлена и не сохранена: для модерации нужен подключённый сервер.";
  });
})();
