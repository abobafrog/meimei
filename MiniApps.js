/* Small browser-only tools for the Frokla page. */
(()=>{
  const dialog=document.getElementById('mini-app-dialog');
  const content=document.getElementById('mini-app-content');
  const title=document.getElementById('mini-app-title');
  const icon=document.getElementById('mini-app-dialog-icon');
  const lamp=document.getElementById('screen-lamp');
  const lampBrightness=document.getElementById('screen-lamp-brightness');
  if(!dialog||!content||!title||!lamp)return;

  const apps={
    assistant:{title:'ИИ-помощник',icon:'✦',html:`<form class="mini-form" id="mini-assistant-form"><label for="mini-assistant-prompt">Кому и какое новогоднее поздравление написать?</label><textarea id="mini-assistant-prompt" maxlength="240" rows="3" placeholder="Например: коротко поздравь бабушку, тепло и с эмодзи"></textarea><button class="mini-action" type="submit">Подготовить открытку <span aria-hidden="true">↗</span></button></form><p class="mini-privacy">Ответ готовит локальная модель Frokla. Текст не отправляется во внешние AI-сервисы.</p><div class="mini-answer" id="mini-assistant-answer" aria-live="polite">Укажи адресата и настроение — подготовлю текст с эмодзи, который можно сразу отправить 🎄</div>`},
    lamp:{title:'Фонарик',icon:'☼',html:`<div class="mini-lamp-copy"><p>Включи подсветку экрана и настрой яркость.</p><button class="mini-action" id="mini-lamp-on" type="button">Включить фонарик <span aria-hidden="true">☼</span></button><p class="mini-privacy">Используется экран телефона, камера не включается.</p></div>`},
    habits:{title:'Трекер привычек',icon:'✓',html:`<form class="mini-form mini-habit-form" id="mini-habit-form"><label for="mini-habit-name">Новая привычка</label><div class="mini-inline-form"><input id="mini-habit-name" maxlength="36" placeholder="Например, прогулка" required><button class="mini-action" type="submit" aria-label="Добавить привычку">+</button></div></form><div class="mini-habit-list" id="mini-habit-list"></div><p class="mini-privacy">Отметки хранятся только в этом браузере.</p>`},
    telegram:{title:'Telegram-бот',icon:'➤',html:`<form class="mini-form" id="mini-telegram-form"><label for="mini-telegram-name">Имя бота в Telegram</label><input id="mini-telegram-name" maxlength="33" autocomplete="off" placeholder="Например, my_frokla_bot"><button class="mini-action" type="submit">Открыть бота <span aria-hidden="true">↗</span></button><p class="mini-inline-error" id="mini-telegram-error" aria-live="polite"></p></form><div class="mini-separator"><span>или</span></div><p>Можно отправить ссылку на Frokla в любой чат.</p><button class="mini-secondary" id="mini-telegram-share" type="button">Поделиться сайтом в Telegram ↗</button><p class="mini-privacy">Имя бота пока не задано в проекте — введи его выше, чтобы перейти к нужному боту.</p>`},
    snowcatch:{title:'Поймай снежинку',icon:'❄',html:`<div class="snowcatch-top"><p>Лови снежинки, пока не закончились 20 секунд.</p><div><span>Счёт <strong id="snowcatch-score">0</strong></span><span>Время <strong id="snowcatch-time">20</strong></span></div></div><div class="snowcatch-stage" id="snowcatch-stage"><button class="snowcatch-target" id="snowcatch-target" type="button" aria-label="Поймать снежинку" hidden>❄</button><p id="snowcatch-message">Готов? Нажми «Начать».</p></div><button class="mini-action" id="snowcatch-start" type="button">Начать игру <span aria-hidden="true">↗</span></button>`},
    focus:{title:'Фокус-таймер',icon:'◷',html:`<p>Один спокойный отрезок для дела — 25 минут.</p><div class="mini-countdown"><div><strong id="focus-time">25:00</strong><span>до перерыва</span></div></div><div class="mini-action-row"><button class="mini-action" id="focus-toggle" type="button">Начать</button><button class="mini-secondary" id="focus-reset" type="button">Сбросить</button></div><p class="mini-privacy" id="focus-status" aria-live="polite">Таймер остановлен.</p>`},
    checklist:{title:'Список дел',icon:'☷',html:`<form class="mini-form" id="mini-check-form"><label for="mini-check-input">Добавить дело</label><div class="mini-inline-form"><input id="mini-check-input" maxlength="80" placeholder="Что нужно сделать?" required><button class="mini-action" type="submit" aria-label="Добавить дело">+</button></div></form><div class="mini-checklist" id="mini-checklist"></div><p class="mini-privacy">Список сохраняется в этом браузере.</p>`},
    mood:{title:'Дневник настроения',icon:'☻',html:`<div data-mood-panel><p>Как ты себя чувствуешь сегодня?</p><div class="mini-mood-options"><button type="button" data-mood="😄" aria-label="Отлично">😄</button><button type="button" data-mood="🙂" aria-label="Хорошо">🙂</button><button type="button" data-mood="😐" aria-label="Нормально">😐</button><button type="button" data-mood="😔" aria-label="Грустно">😔</button><button type="button" data-mood="😴" aria-label="Устал">😴</button></div><label for="mini-mood-note">Заметка (необязательно)</label><textarea class="mini-mood-note" id="mini-mood-note" maxlength="160" placeholder="Что хочется запомнить?"></textarea><button class="mini-action" id="mini-mood-save" type="button">Сохранить за сегодня</button><div class="mini-output" id="mini-mood-status" aria-live="polite"></div></div><h3 class="mini-history-title">Последние записи</h3><div class="mini-mood-history" id="mini-mood-history"></div><p class="mini-privacy">Записи остаются на этом устройстве.</p>`},
    countdown:{title:'До Нового года',icon:'⌛',html:`<p>До следующего Нового года осталось:</p><div class="mini-countdown" id="mini-newyear-countdown"><div><strong>—</strong><span>дней</span></div><div><strong>—</strong><span>часов</span></div><div><strong>—</strong><span>минут</span></div><div><strong>—</strong><span>секунд</span></div></div><p class="mini-privacy">Счётчик обновляется по времени твоего устройства.</p>`},
    password:{title:'Генератор паролей',icon:'⌑',html:`<form class="mini-form" id="mini-password-form"><label for="mini-password-length">Длина: <output id="mini-password-size">20</output></label><input id="mini-password-length" type="range" min="8" max="48" value="20"><div class="mini-password-options"><label><input id="mini-password-symbols" type="checkbox" checked> Добавить символы</label></div><button class="mini-action" type="submit">Создать пароль ↻</button></form><div class="mini-output mini-password-output" id="mini-password-output" aria-live="polite"></div><button class="mini-secondary" id="mini-password-copy" type="button">Скопировать пароль</button><p class="mini-privacy">Пароль создаётся на устройстве и никуда не отправляется.</p>`},
    calculator:{title:'Калькулятор',icon:'＋',html:`<form class="mini-form" id="mini-calculator-form"><label for="mini-calculation">Вычисление</label><input id="mini-calculation" inputmode="decimal" autocomplete="off" placeholder="Например: (125 + 37) × 2"><button class="mini-action" type="submit">Посчитать =</button></form><div class="mini-output" id="mini-calculation-result" aria-live="polite"></div><p class="mini-privacy">Поддерживаются числа, скобки, +, −, ×, ÷ и остаток от деления (%).</p>`},
    texttools:{title:'Инструменты текста',icon:'Aa',html:`<label for="mini-text-input">Текст</label><textarea class="mini-mood-note" id="mini-text-input" maxlength="10000" rows="6" placeholder="Вставь или напиши текст"></textarea><div class="mini-text-stats" id="mini-text-stats">0 символов · 0 слов</div><div class="mini-action-row"><button class="mini-secondary" id="mini-text-upper" type="button">В ВЕРХНИЙ РЕГИСТР</button><button class="mini-secondary" id="mini-text-lower" type="button">в нижний регистр</button><button class="mini-secondary" id="mini-text-copy" type="button">Скопировать</button></div><p class="mini-privacy" id="mini-text-status" aria-live="polite"></p>`},
    converter:{title:'Конвертер величин',icon:'↔',html:`<label for="mini-converter-type">Что переводим?</label><select class="mini-gift-select" id="mini-converter-type"><option value="length">Длина · метры и километры</option><option value="weight">Вес · граммы и килограммы</option><option value="temperature">Температура · °C и °F</option></select><div class="mini-inline-form"><input id="mini-converter-value" type="number" step="any" placeholder="Значение"><select class="mini-gift-select" id="mini-converter-direction"><option value="forward">из первой единицы во вторую</option><option value="backward">в обратную сторону</option></select></div><div class="mini-output" id="mini-converter-result" aria-live="polite">Введи число для пересчёта.</div>`},
    notes:{title:'Быстрые заметки',icon:'▤',html:`<label for="mini-notes-text">Заметка</label><textarea class="mini-mood-note" id="mini-notes-text" maxlength="5000" rows="7" placeholder="Запиши, чтобы не забыть"></textarea><div class="mini-action-row"><button class="mini-action" id="mini-notes-save" type="button">Сохранить</button><button class="mini-secondary" id="mini-notes-clear" type="button">Очистить</button></div><p class="mini-privacy" id="mini-notes-status" aria-live="polite">Заметка хранится в этом браузере.</p>`},
    budget:{title:'Учёт расходов',icon:'₽',html:`<form class="mini-form" id="mini-budget-form"><label for="mini-budget-title">Покупка или расход</label><input id="mini-budget-title" maxlength="60" placeholder="Например, продукты" required><label for="mini-budget-amount">Сумма, ₽</label><div class="mini-inline-form"><input id="mini-budget-amount" type="number" min="0.01" step="0.01" inputmode="decimal" placeholder="0" required><button class="mini-action" type="submit" aria-label="Добавить расход">+</button></div></form><div class="mini-output" id="mini-budget-total">Расходы: 0 ₽</div><div class="mini-checklist" id="mini-budget-list"></div><p class="mini-privacy">Суммы хранятся только в этом браузере.</p>`}
  };
  const meiTips={assistant:'Помогу подготовить открытку к отправке 🎄',lamp:'Добавим немного света и уюта ✨',habits:'Маленький шаг — и привычка крепче 🌱',telegram:'Помогу найти нужный бот ➤',snowcatch:'Лови снежинки, я болею за тебя ❄',focus:'Пора спокойно сосредоточиться ⏳',checklist:'Разберём дела по одному ✓',mood:'Как ты сегодня? Я рядом ☺',countdown:'До праздника уже совсем немного 🎄',password:'Пусть пароль будет надёжным 🔐',calculator:'Сейчас всё точно посчитаем ✨',texttools:'Приведём текст в порядок Aa',converter:'Переведём всё без путаницы ↔',notes:'Запиши идею, чтобы не потерять ✍',budget:'Следим за расходами бережно ₽'};
  const escapeHTML=value=>String(value).replace(/[&<>"']/g,char=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  const storeKeys={habits:'frokla.habits.v1',checklist:'frokla.checklist.v1',mood:'frokla.mood.v1',notes:'frokla.notes.v1',budget:'frokla.budget.v1'};
  const menu=document.getElementById('mini-service-menu');
  let gameTimer=0,moveTimer=0,gameScore=0,gameTime=20,serviceTimers=[],focusTimer=0,focusSeconds=1500,focusRunning=false;
  function clearActivities(){stopGame();serviceTimers.forEach(clearInterval);serviceTimers=[];clearInterval(focusTimer);focusTimer=0;focusRunning=false}
  function addInterval(fn,ms){const timer=setInterval(fn,ms);serviceTimers.push(timer);return timer}

  function stopGame(){clearInterval(gameTimer);clearInterval(moveTimer);gameTimer=moveTimer=0}
  function closeDialog(){clearActivities();if(dialog.open)dialog.close();else dialog.removeAttribute('open');lampOff()}
  let activeService='',lastUsage=0;
  function usage(action){if(!activeService||window.froklaLocal)return;const now=Date.now();if(action==='interact'&&now-lastUsage<2000)return;lastUsage=now;const service=activeService;Promise.resolve(window.froklaVisitReady).then(()=>fetch('/api/service-event',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({service,action}),keepalive:true})).catch(()=>{})}
  content.addEventListener('submit',()=>usage('submit'),true);
  content.addEventListener('click',e=>{if(e.target.closest('button:not([type="submit"])'))usage('interact')},true);
  function openApp(key){const app=apps[key];if(!app)return;activeService=key;usage('open');clearActivities();if(menu)menu.open=false;title.textContent=app.title;icon.textContent=app.icon;content.innerHTML=`<aside class="mini-mei-helper"><img src="mini-mei-holiday.png" alt=""><span><strong>Мей-Мей</strong>${meiTips[key]||'Я рядом, если понадобится помощь ✨'}</span></aside>${app.html}`;if(typeof dialog.showModal==='function')dialog.showModal();else dialog.setAttribute('open','');if(key==='habits')renderHabits();if(key==='checklist')renderChecklist();if(key==='mood')renderMood();if(key==='notes')loadNotes();if(key==='budget')renderBudget();if(key==='countdown')startCountdown();if(key==='focus')renderFocus();if(key==='password')document.getElementById('mini-password-output').textContent=makePassword();if(key==='texttools')updateTextStats();if(key==='converter')convertUnits();content.querySelector('textarea,input')?.focus({preventScroll:true})}
  function lampOn(){if(typeof lamp.showModal==='function')lamp.showModal();else lamp.setAttribute('open','');document.body.classList.add('screen-lamp-active');lamp.setAttribute('aria-hidden','false');lamp.focus?.();}
  function lampOff(){if(lamp.open)lamp.close();else lamp.removeAttribute('open');document.body.classList.remove('screen-lamp-active');lamp.setAttribute('aria-hidden','true')}
  async function askAssistant(prompt){
    const answer=document.getElementById('mini-assistant-answer'),button=document.querySelector('#mini-assistant-form [type="submit"]');
    if(window.froklaLocal){answer.textContent='Чтобы поговорить с моделью, запусти Frokla через Rust-сервер: cargo run --release.';return}
    button.disabled=true;answer.textContent='Модель думает…';
    try{
      const response=await window.froklaRequest('/api/assistant',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({prompt})});
      const data=await response.json().catch(()=>({}));
      if(!response.ok)throw Error(data.error||'Сервер не смог ответить.');
      answer.textContent=typeof data.reply==='string'&&data.reply.trim()?data.reply:'Модель не выдала ответ. Попробуй ещё раз.';
    }catch(error){answer.textContent=error.message||'Не удалось связаться с локальной моделью.'}
    finally{button.disabled=false}
  }
  function localDate(date=new Date()){return `${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,'0')}-${String(date.getDate()).padStart(2,'0')}`}
  function readHabits(){try{const data=JSON.parse(localStorage.getItem(storeKeys.habits)||'[]');return Array.isArray(data)?data.filter(item=>item&&typeof item.id==='string'&&typeof item.title==='string'&&item.marks&&typeof item.marks==='object'):[]}catch{return []}}
  function writeHabits(habits){try{localStorage.setItem(storeKeys.habits,JSON.stringify(habits));return true}catch{return false}}
  function streakFor(marks){const today=localDate(),cursor=new Date();if(!marks[today])cursor.setDate(cursor.getDate()-1);let count=0;for(let i=0;i<366;i++){if(!marks[localDate(cursor)])break;count++;cursor.setDate(cursor.getDate()-1)}return count}
  function renderHabits(){const list=document.getElementById('mini-habit-list');if(!list)return;const habits=readHabits(),today=localDate();if(!habits.length){list.innerHTML='<p class="mini-empty">Добавь первую привычку — отмечай её каждый день.</p>';return}list.innerHTML=habits.map(habit=>`<div class="mini-habit-row"><label><input type="checkbox" data-habit-toggle="${escapeHTML(habit.id)}" ${habit.marks[today]?'checked':''}><span>${escapeHTML(habit.title)}</span></label><small>${streakFor(habit.marks)} дн. подряд</small><button type="button" data-habit-delete="${escapeHTML(habit.id)}" aria-label="Удалить привычку ${escapeHTML(habit.title)}">×</button></div>`).join('')}
  function readStored(key,fallback){try{return JSON.parse(localStorage.getItem(key)??JSON.stringify(fallback))}catch{return fallback}}
  function writeStored(key,value){try{localStorage.setItem(key,JSON.stringify(value));return true}catch{return false}}
  function readArray(key){const value=readStored(key,[]);return Array.isArray(value)?value:[]}
  function readObject(key){const value=readStored(key,{});return value&&typeof value==='object'&&!Array.isArray(value)?value:{}}
  function renderChecklist(){const list=document.getElementById('mini-checklist');if(!list)return;const rows=readArray(storeKeys.checklist);list.innerHTML=rows.length?rows.map(row=>`<div class="mini-check-row"><label><input type="checkbox" data-check-toggle="${escapeHTML(row.id)}" ${row.done?'checked':''}><span>${escapeHTML(row.text)}</span></label><button type="button" data-check-delete="${escapeHTML(row.id)}" aria-label="Удалить дело">×</button></div>`).join(''):'<p class="mini-empty">Список пока пуст.</p>'}
  function renderMood(){const panel=content.querySelector('[data-mood-panel]');if(!panel)return;const records=readObject(storeKeys.mood),record=records[localDate()];panel.dataset.selectedMood=record?.mood||'';content.querySelectorAll('[data-mood]').forEach(button=>button.setAttribute('aria-pressed',String(button.dataset.mood===record?.mood)));const note=document.getElementById('mini-mood-note');note.value=record?.note||'';document.getElementById('mini-mood-status').textContent=record?`Сохранено на сегодня: ${record.mood}`:'Выбери настроение и сохрани запись.';const history=document.getElementById('mini-mood-history'),days=Object.keys(records).sort().reverse().slice(0,7);history.innerHTML=days.length?days.map(date=>`<div><time>${escapeHTML(date)}</time><span>${escapeHTML(records[date].mood||'')}</span><small>${escapeHTML(records[date].note||'')}</small></div>`).join(''):'<p class="mini-empty">Записей пока нет.</p>'}
  function loadNotes(){document.getElementById('mini-notes-text').value=readStored(storeKeys.notes,'')}
  function renderBudget(){const list=document.getElementById('mini-budget-list'),total=document.getElementById('mini-budget-total');if(!list||!total)return;const rows=readArray(storeKeys.budget),sum=rows.reduce((acc,row)=>acc+(Number(row.amount)||0),0);total.textContent=`Расходы: ${new Intl.NumberFormat('ru-RU',{maximumFractionDigits:2}).format(sum)} ₽`;list.innerHTML=rows.length?rows.slice().reverse().map(row=>`<div class="mini-check-row"><span>${escapeHTML(row.title)} · ${new Intl.NumberFormat('ru-RU',{maximumFractionDigits:2}).format(row.amount)} ₽</span><button type="button" data-budget-delete="${escapeHTML(row.id)}" aria-label="Удалить расход">×</button></div>`).join(''):'<p class="mini-empty">Добавь первый расход.</p>'}
  function formatFocusTime(){const node=document.getElementById('focus-time');if(node)node.textContent=`${String(Math.floor(focusSeconds/60)).padStart(2,'0')}:${String(focusSeconds%60).padStart(2,'0')}`}
  function renderFocus(){formatFocusTime();const button=document.getElementById('focus-toggle'),status=document.getElementById('focus-status');if(button)button.textContent=focusRunning?'Пауза':'Начать';if(status)status.textContent=focusRunning?'Идёт фокус-сессия.':'Таймер остановлен.'}
  function startFocus(){clearInterval(focusTimer);focusTimer=setInterval(()=>{if(focusSeconds>0)focusSeconds--;formatFocusTime();if(focusSeconds<=0){clearInterval(focusTimer);focusTimer=0;focusRunning=false;renderFocus();const status=document.getElementById('focus-status');if(status)status.textContent='Время фокус-сессии вышло — пора сделать перерыв.'}},1000)}
  function startCountdown(){const update=()=>{const now=new Date(),target=new Date(now.getFullYear()+1,0,1),seconds=Math.max(0,Math.floor((target-now)/1000)),parts=[Math.floor(seconds/86400),Math.floor(seconds%86400/3600),Math.floor(seconds%3600/60),seconds%60],nodes=document.querySelectorAll('#mini-newyear-countdown strong');parts.forEach((value,index)=>{if(nodes[index])nodes[index].textContent=String(value).padStart(index?'2':'1','0')})};update();addInterval(update,1000)}
  function secureIndex(limit){const ceiling=Math.floor(0x100000000/limit)*limit,buffer=new Uint32Array(1);let value;do{crypto.getRandomValues(buffer);value=buffer[0]}while(value>=ceiling);return value%limit}
  function makePassword(){const length=Number(document.getElementById('mini-password-length')?.value)||20,symbols=document.getElementById('mini-password-symbols')?.checked,alphabet='ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789'+(symbols?'!@#$%^&*()-_=+[]{}?':'');return Array.from({length},()=>alphabet[secureIndex(alphabet.length)]).join('')}
  async function copyText(value,status){try{await navigator.clipboard.writeText(value);status.textContent='Скопировано.'}catch{status.textContent='Буфер обмена недоступен в этом режиме.'}}
  function calculate(expression){const source=expression.replace(/[×x]/gi,'*').replace(/[÷]/g,'/').replace(/[−–]/g,'-').replace(/,/g,'.').replace(/\s/g,''),tokens=source.match(/(?:\d+(?:\.\d*)?|\.\d+)|[()+\-*/%]/g);if(!tokens?.length||tokens.join('')!==source)throw Error('Проверь выражение.');let index=0;function primary(){const token=tokens[index++];if(token==='+'||token==='-'){const number=primary();return token==='-'?-number:number}if(token==='('){const value=additive();if(tokens[index++]!==')')throw Error('Не закрыта скобка.');return value}const value=Number(token);if(!Number.isFinite(value)||token===undefined)throw Error('Проверь число.');return value}function multiplicative(){let value=primary();while(['*','/','%'].includes(tokens[index])){const op=tokens[index++],next=primary();if((op==='/'||op==='%')&&next===0)throw Error('На ноль делить нельзя.');value=op==='*'?value*next:op==='/'?value/next:value%next}return value}function additive(){let value=multiplicative();while(tokens[index]==='+'||tokens[index]==='-'){const op=tokens[index++],next=multiplicative();value=op==='+'?value+next:value-next}return value}const result=additive();if(index!==tokens.length||!Number.isFinite(result))throw Error('Проверь выражение.');return result}
  function convertUnits(){const type=document.getElementById('mini-converter-type').value,raw=document.getElementById('mini-converter-value').value,value=Number(raw),backward=document.getElementById('mini-converter-direction').value==='backward',result=document.getElementById('mini-converter-result');if(!raw.trim()||!Number.isFinite(value)){result.textContent='Введи число для пересчёта.';return}let output,units;if(type==='length'){output=backward?value*1000:value/1000;units=backward?'км → м':'м → км'}else if(type==='weight'){output=backward?value*1000:value/1000;units=backward?'кг → г':'г → кг'}else{output=backward?(value-32)*5/9:value*9/5+32;units=backward?'°F → °C':'°C → °F'}result.textContent=`${new Intl.NumberFormat('ru-RU',{maximumFractionDigits:6}).format(value)} ${units.split(' → ')[0]} = ${new Intl.NumberFormat('ru-RU',{maximumFractionDigits:6}).format(output)} ${units.split(' → ')[1]}`}
  function updateTextStats(){const value=document.getElementById('mini-text-input')?.value||'',words=value.trim()?value.trim().split(/\s+/u).length:0,node=document.getElementById('mini-text-stats');if(node)node.textContent=`${[...value].length} символов · ${words} слов`}
  function moveTarget(){const target=document.getElementById('snowcatch-target');if(!target)return;target.style.left=`${5+Math.random()*78}%`;target.style.top=`${8+Math.random()*68}%`}
  function startGame(){stopGame();gameScore=0;gameTime=20;const score=document.getElementById('snowcatch-score'),time=document.getElementById('snowcatch-time'),target=document.getElementById('snowcatch-target'),message=document.getElementById('snowcatch-message'),button=document.getElementById('snowcatch-start');score.textContent='0';time.textContent='20';message.textContent='Лови!';button.textContent='Игра идёт…';button.disabled=true;target.hidden=false;moveTarget();moveTimer=setInterval(moveTarget,720);gameTimer=setInterval(()=>{gameTime--;time.textContent=String(gameTime);if(gameTime<=0){stopGame();target.hidden=true;message.textContent=`Время! Ты поймал снежинок: ${gameScore}.`;button.textContent='Сыграть ещё раз';button.disabled=false}},1000)}

  document.addEventListener('click',event=>{
    const open=event.target.closest('[data-mini-app]');
    if(open){openApp(open.dataset.miniApp);return}
    if(event.target.closest('[data-mini-close]')){closeDialog();return}
    if(menu?.open&&!menu.contains(event.target))menu.open=false;
  });
  dialog.addEventListener('click',event=>{if(event.target===dialog)closeDialog()});
  dialog.addEventListener('close',()=>{clearActivities();lampOff()});
  dialog.addEventListener('cancel',()=>{clearActivities();lampOff()});
  content.addEventListener('submit',async event=>{
    const form=event.target;
    if(form.id==='mini-assistant-form'){
      event.preventDefault();
      const prompt=document.getElementById('mini-assistant-prompt').value.trim();
      if(prompt)await askAssistant(prompt);else document.getElementById('mini-assistant-answer').textContent='Напиши вопрос или тему для помощника.';
    }
    if(form.id==='mini-habit-form'){
      event.preventDefault();const input=document.getElementById('mini-habit-name'),name=input.value.trim();if(!name)return;
      const habits=readHabits();habits.push({id:`h-${Date.now()}-${Math.random().toString(36).slice(2,7)}`,title:name,marks:{}});
      if(!writeHabits(habits)){content.querySelector('.mini-privacy').textContent='Не удалось сохранить привычку. Проверь свободное место в браузере.';return}
      input.value='';renderHabits();
    }
    if(form.id==='mini-telegram-form'){
      event.preventDefault();const input=document.getElementById('mini-telegram-name'),error=document.getElementById('mini-telegram-error'),handle=input.value.trim().replace(/^@/,'');
      if(!/^[A-Za-z0-9_]{5,32}$/.test(handle)){error.textContent='Введи имя бота: от 5 до 32 латинских букв, цифр или _.';return}
      error.textContent='';window.open(`https://t.me/${encodeURIComponent(handle)}`,'_blank','noopener,noreferrer');
    }
    if(form.id==='mini-check-form'){
      event.preventDefault();const input=document.getElementById('mini-check-input'),text=input.value.trim();if(!text)return;
      const stored=readArray(storeKeys.checklist),rows=Array.isArray(stored)?stored:[];rows.push({id:`c-${Date.now()}`,text,done:false});
      if(writeStored(storeKeys.checklist,rows)){input.value='';renderChecklist()}
    }
    if(form.id==='mini-budget-form'){
      event.preventDefault();const titleInput=document.getElementById('mini-budget-title'),amountInput=document.getElementById('mini-budget-amount'),amount=Number(amountInput.value),name=titleInput.value.trim();if(!name||!Number.isFinite(amount)||amount<=0)return;
      const stored=readArray(storeKeys.budget),rows=Array.isArray(stored)?stored:[];rows.push({id:`b-${Date.now()}-${Math.random().toString(36).slice(2,5)}`,title:name,amount:Math.round(amount*100)/100});
      if(writeStored(storeKeys.budget,rows)){titleInput.value='';amountInput.value='';renderBudget()}
    }
    if(form.id==='mini-password-form'){event.preventDefault();document.getElementById('mini-password-output').textContent=makePassword()}
    if(form.id==='mini-calculator-form'){
      event.preventDefault();const result=document.getElementById('mini-calculation-result');
      try{result.textContent=new Intl.NumberFormat('ru-RU',{maximumFractionDigits:10}).format(calculate(document.getElementById('mini-calculation').value))}catch(error){result.textContent=error.message}
    }
  });
  content.addEventListener('click',event=>{
    if(event.target.closest('#mini-lamp-on')){lampOn();return}
    if(event.target.closest('#snowcatch-start')){startGame();return}
    if(event.target.closest('#snowcatch-target')&&gameTimer){gameScore++;document.getElementById('snowcatch-score').textContent=String(gameScore);moveTarget();return}
    if(event.target.closest('#mini-telegram-share')){const url=encodeURIComponent(location.href.split('#')[0]),text=encodeURIComponent('Загляни в Frokla — зимний уголок с посланиями и рисунками.');window.open(`https://t.me/share/url?url=${url}&text=${text}`,'_blank','noopener,noreferrer');return}
    const habitRemove=event.target.closest('[data-habit-delete]');
    if(habitRemove){writeHabits(readHabits().filter(habit=>habit.id!==habitRemove.dataset.habitDelete));renderHabits();return}
    const checkRemove=event.target.closest('[data-check-delete]');
    if(checkRemove){const stored=readArray(storeKeys.checklist);writeStored(storeKeys.checklist,(Array.isArray(stored)?stored:[]).filter(row=>row.id!==checkRemove.dataset.checkDelete));renderChecklist();return}
    const budgetRemove=event.target.closest('[data-budget-delete]');
    if(budgetRemove){const stored=readArray(storeKeys.budget);writeStored(storeKeys.budget,(Array.isArray(stored)?stored:[]).filter(row=>row.id!==budgetRemove.dataset.budgetDelete));renderBudget();return}
    if(event.target.closest('#focus-toggle')){
      if(focusRunning){clearInterval(focusTimer);focusTimer=0;focusRunning=false}else{if(!focusSeconds)focusSeconds=1500;focusRunning=true;startFocus()}
      renderFocus();return;
    }
    if(event.target.closest('#focus-reset')){clearInterval(focusTimer);focusTimer=0;focusRunning=false;focusSeconds=1500;renderFocus();return}
    const mood=event.target.closest('[data-mood]');
    if(mood){const panel=content.querySelector('[data-mood-panel]');panel.dataset.selectedMood=mood.dataset.mood;content.querySelectorAll('[data-mood]').forEach(button=>button.setAttribute('aria-pressed',String(button===mood)));return}
    if(event.target.closest('#mini-mood-save')){
      const panel=content.querySelector('[data-mood-panel]'),chosen=panel.dataset.selectedMood,status=document.getElementById('mini-mood-status');if(!chosen){status.textContent='Сначала выбери настроение.';return}
      const data=readObject(storeKeys.mood);data[localDate()]={mood:chosen,note:document.getElementById('mini-mood-note').value.trim()};status.textContent=writeStored(storeKeys.mood,data)?`Запись за сегодня сохранена: ${chosen}`:'Не удалось сохранить запись.';return;
    }
    if(event.target.closest('#mini-notes-save')){const status=document.getElementById('mini-notes-status');status.textContent=writeStored(storeKeys.notes,document.getElementById('mini-notes-text').value)?'Заметка сохранена на этом устройстве.':'Не удалось сохранить заметку.';return}
    if(event.target.closest('#mini-notes-clear')){document.getElementById('mini-notes-text').value='';writeStored(storeKeys.notes,'');document.getElementById('mini-notes-status').textContent='Заметка очищена.';return}
    if(event.target.closest('#mini-password-copy')){copyText(document.getElementById('mini-password-output').textContent,content.querySelector('.mini-privacy'));return}
    if(event.target.closest('#mini-text-upper')){const input=document.getElementById('mini-text-input');input.value=input.value.toLocaleUpperCase('ru-RU');updateTextStats();return}
    if(event.target.closest('#mini-text-lower')){const input=document.getElementById('mini-text-input');input.value=input.value.toLocaleLowerCase('ru-RU');updateTextStats();return}
    if(event.target.closest('#mini-text-copy')){copyText(document.getElementById('mini-text-input').value,document.getElementById('mini-text-status'));return}

  });
  content.addEventListener('change',event=>{
    const habitBox=event.target.closest('[data-habit-toggle]');
    if(habitBox){const habits=readHabits(),habit=habits.find(item=>item.id===habitBox.dataset.habitToggle);if(!habit)return;const today=localDate();if(habitBox.checked)habit.marks[today]=true;else delete habit.marks[today];writeHabits(habits);renderHabits();return}
    const checkBox=event.target.closest('[data-check-toggle]');
    if(checkBox){const stored=readArray(storeKeys.checklist),rows=Array.isArray(stored)?stored:[],row=rows.find(item=>item.id===checkBox.dataset.checkToggle);if(row){row.done=checkBox.checked;writeStored(storeKeys.checklist,rows);renderChecklist()}return}
    if(event.target.id==='mini-converter-type'||event.target.id==='mini-converter-direction')convertUnits();
    if(event.target.id==='mini-password-length')document.getElementById('mini-password-size').textContent=event.target.value;
  });
  content.addEventListener('input',event=>{
    if(event.target.id==='mini-text-input')updateTextStats();
    if(['mini-converter-value','mini-converter-type','mini-converter-direction'].includes(event.target.id))convertUnits();
  });
  document.getElementById('screen-lamp-off')?.addEventListener('click',lampOff);
  lamp.addEventListener('close',()=>{document.body.classList.remove('screen-lamp-active');lamp.setAttribute('aria-hidden','true')});
  lampBrightness?.addEventListener('input',()=>lamp.style.setProperty('--lamp-brightness',String(Number(lampBrightness.value)/100)));
  document.addEventListener('keydown',event=>{if(event.key!=='Escape')return;if(lamp.open)lampOff();if(menu?.open){menu.open=false;menu.querySelector('summary')?.focus()}});
})();
