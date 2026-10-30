// Блок «Ранний старт» (B9) для окна настроек и онбординга: переключатель, «Хочу сброс в»,
// время задачи, проверка по данным лимита и пошаговая инструкция «Настроить задачу в Claude».
// Расчёты — early.js. Изменения отдаёт наружу через opts.onChange(patch).
(function () {
  "use strict";
  const TC = (window.TokenCat = window.TokenCat || {});
  const E = () => TC.early;

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  /** Скопировать текст: буфер обмена, а если он недоступен — выделить поле для Ctrl+C. */
  async function copy(text, input, btn) {
    let ok = false;
    try { await navigator.clipboard.writeText(text); ok = true; } catch (_) {}
    if (!ok && input) {
      input.focus();
      input.select();
      try { ok = document.execCommand("copy"); } catch (_) {}
    }
    const was = btn.textContent;
    btn.textContent = ok ? "Скопировано" : "Нажмите Ctrl+C";
    setTimeout(() => { btn.textContent = was; }, 1600);
  }

  /** Строка «текст + Копировать». */
  function copyRow(text) {
    const row = el("div", "copy-row");
    const inp = el("input");
    inp.type = "text";
    inp.readOnly = true;
    inp.value = text;
    const b = el("button", null, "Копировать");
    b.type = "button";
    b.onclick = () => copy(inp.value, inp, b);
    row.append(inp, b);
    return { row, set: (t) => { inp.value = t; } };
  }

  class EarlyStart {
    /**
     * opts.onChange(patch) — сохранить изменение (Promise);
     * opts.status=true — показывать проверку «сработало / не сработало» (нужен setUsage).
     */
    constructor(opts = {}) {
      this.opts = opts;
      this.st = { early_start: true, early_reset_hour: 11 };
      this.session = null;
      this.haveData = false;
      const root = (this.root = el("div", "early"));

      // Переключатель
      const sw = el("div", "item");
      const swText = el("div", "text", "Ранний старт 5-часового окна");
      swText.appendChild(el("small", null, "Лимит обновится посреди рабочего дня, а не к вечеру."));
      const lab = el("label", "switch");
      this.on = el("input");
      this.on.type = "checkbox";
      lab.append(this.on, el("i"));
      sw.append(swText, lab);
      this.on.onchange = () => this.change({ early_start: this.on.checked });

      // Час сброса
      const hr = (this.hourRow = el("div", "item"));
      const hrText = el("div", "text", "Хочу сброс в");
      this.taskLine = el("small");
      hrText.appendChild(this.taskLine);
      this.hour = el("select");
      for (let h = 0; h < 24; h++) {
        const o = el("option", null, E().hhmm(h, 0));
        o.value = String(h);
        this.hour.appendChild(o);
      }
      this.hour.onchange = () => this.change({ early_reset_hour: Number(this.hour.value) });
      hr.append(hrText, this.hour);

      // Проверка по данным
      this.statusRow = el("p", "msg item");
      this.statusRow.hidden = true;

      // Инструкция
      const g = (this.guideRow = el("div", "item col"));
      this.guideBtn = el("button", "primary", "Настроить задачу в Claude");
      this.guideBtn.type = "button";
      this.guideBtn.onclick = () => {
        this.guide.hidden = !this.guide.hidden;
        this.guideBtn.textContent = this.guide.hidden ? "Настроить задачу в Claude" : "Скрыть инструкцию";
      };
      const btnRow = el("div", "row-inline");
      btnRow.appendChild(this.guideBtn);
      this.guide = this.buildGuide();
      this.guide.hidden = true;
      g.append(btnRow, this.guide);

      root.append(sw, hr, this.statusRow, g);
      this.render();
    }

    buildGuide() {
      const box = el("div", "guide");
      const ol = el("ol");
      const li = (html) => { const x = el("li"); x.innerHTML = html; ol.appendChild(x); return x; };
      li("Откройте Claude (приложение или claude.ai) и слева выберите <b>«Запланированные»</b> " +
         "(Scheduled) → <b>«Новая задача»</b> → <b>«Настроить вручную»</b>.");
      li("Название — <b>Котофакт</b>. В поле запроса вставьте:").appendChild(copyRow(E().PROMPT).row);
      this.timeStep = li("");
      li("Модель — самую лёгкую из списка (например Haiku), если можно выбрать. Нажмите <b>«Сохранить»</b>.");
      box.appendChild(ol);
      const alt = el("p", "small muted", "Или просто отправьте это сообщение Claude в новом чате — он сам создаст задачу:");
      this.altCopy = copyRow("");
      box.append(alt, this.altCopy.row);
      box.appendChild(el("p", "small muted",
        "Задача выполняется на стороне Claude. Запрос крошечный — лимит почти не тратит. " +
        "TokenCat ничего не отправляет сам: он только покажет, сработал ли ранний старт."));
      return box;
    }

    change(patch) {
      Object.assign(this.st, patch);
      this.render();
      if (this.opts.onChange) Promise.resolve(this.opts.onChange(patch)).catch(() => {});
    }

    /** Новые настройки (SettingsView). */
    set(st) {
      if (!st) return;
      this.st = { early_start: st.early_start !== false, early_reset_hour: st.early_reset_hour ?? 11 };
      this.render();
    }

    /** Снимок планировщика — для проверки «сработало ли». */
    setUsage(snap) {
      const u = snap && snap.usage;
      this.haveData = !!u;
      this.session = (u && u.session) || null;
      this.render();
    }

    render() {
      const e = E(), on = this.st.early_start, h = this.st.early_reset_hour;
      const task = e.taskLabel(h);
      this.on.checked = on;
      this.hour.value = String(h);
      this.hourRow.hidden = !on;
      this.guideRow.hidden = !on;
      const prev = e.taskTime(h).prevDay ? " накануне" : "";
      this.taskLine.textContent = `Задача отправит короткий запрос в ${task}${prev}. Ставьте сброс на 2–3 часа позже начала работы.`;
      this.timeStep.innerHTML = `Как часто — <b>каждый день</b>, время — <b>${task}</b>` +
        (prev ? " (вечером накануне)" : "") + ". Если минуты выбрать нельзя — ставьте ровно " +
        `<b>${e.hhmm(e.taskTime(h).h, 0)}</b>.`;
      this.altCopy.set(`Создай запланированную задачу «Котофакт»: каждый день в ${task} по моему местному времени ` +
        `отправляй запрос «${e.PROMPT}» Используй самую лёгкую модель.`);
      if (this.opts.status) {
        const s = e.status({ enabled: on, resetHour: h, session: this.session, haveData: this.haveData });
        this.statusRow.hidden = !on || !s.text;
        this.statusRow.textContent = s.text;
        this.statusRow.className = "msg item " + ({ ok: "ok", miss: "warn", other: "warn" }[s.kind] || "muted");
      }
    }
  }

  TC.ui = TC.ui || {};
  TC.ui.EarlyStart = EarlyStart;
})();
