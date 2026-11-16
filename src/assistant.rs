use anyhow::{bail, Context, Result};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, LlamaModel},
    sampling::LlamaSampler,
};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufReader, Read},
    num::NonZeroU32,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::{mpsc, oneshot},
};

const MODEL_REPOSITORY: &str = "Qwen/Qwen2.5-1.5B-Instruct-GGUF";
const MODEL_NAME: &str = "qwen2.5-1.5b-instruct-q4_k_m.gguf";
const MODEL_URL: &str = "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf";
const MODEL_SHA256: &str = "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e";
const MAX_MODEL_BYTES: u64 = 1250 * 1024 * 1024;
const LEGACY_MODEL_NAME: &str = "qwen2.5-0.5b-instruct-q4_k_m.gguf";
const LEGACY_MODEL_SHA256: &str =
    "74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db";
const LEGACY_MAX_MODEL_BYTES: u64 = 600 * 1024 * 1024;
const CONTEXT_TOKENS: u32 = 2048;
const MAX_GENERATED_TOKENS: usize = 120;

pub type AssistantReply = std::result::Result<String, String>;

struct Job {
    prompt: String,
    answer: oneshot::Sender<AssistantReply>,
}

#[derive(Clone)]
pub struct AssistantService {
    tx: mpsc::Sender<Job>,
}

impl AssistantService {
    pub fn try_submit(&self, prompt: String) -> Result<oneshot::Receiver<AssistantReply>, ()> {
        let (answer, receiver) = oneshot::channel();
        self.tx.try_send(Job { prompt, answer }).map_err(|_| ())?;
        Ok(receiver)
    }
}

pub fn model_path(root: &Path) -> PathBuf {
    root.join("models").join("assistant").join(MODEL_NAME)
}

fn legacy_model_path(root: &Path) -> PathBuf {
    root.join("models")
        .join("assistant")
        .join(LEGACY_MODEL_NAME)
}

/// Downloads the pinned, quantized assistant model on first setup and verifies it before use.
pub async fn ensure_model(root: &Path) -> Result<PathBuf> {
    let destination = model_path(root);
    if verify_model(destination.clone()).await? {
        tracing::info!(
            model = MODEL_NAME,
            "local assistant model is already installed"
        );
        return Ok(destination);
    }

    if fs::try_exists(&destination).await.unwrap_or(false) {
        fs::remove_file(&destination)
            .await
            .with_context(|| format!("remove invalid model file {}", destination.display()))?;
    }
    let directory = destination
        .parent()
        .context("assistant model destination has no parent directory")?;
    fs::create_dir_all(directory)
        .await
        .with_context(|| format!("create model directory {}", directory.display()))?;
    let partial = destination.with_extension("gguf.part");
    let _ = fs::remove_file(&partial).await;

    tracing::info!(
        model = MODEL_NAME,
        repository = MODEL_REPOSITORY,
        "downloading improved local assistant model (~1.12 GB)"
    );
    if let Err(error) = download_model(&partial).await {
        let _ = fs::remove_file(&partial).await;
        let legacy = legacy_model_path(root);
        if verify_model_checksum(legacy.clone(), LEGACY_MODEL_SHA256, LEGACY_MAX_MODEL_BYTES)
            .await?
        {
            tracing::warn!(%error, "using the installed smaller assistant model because the improved model could not be downloaded");
            return Ok(legacy);
        }
        return Err(error);
    }
    if let Err(error) = fs::rename(&partial, &destination)
        .await
        .with_context(|| format!("install model at {}", destination.display()))
    {
        let legacy = legacy_model_path(root);
        if verify_model_checksum(legacy.clone(), LEGACY_MODEL_SHA256, LEGACY_MAX_MODEL_BYTES)
            .await?
        {
            tracing::warn!(%error, "using the installed smaller assistant model because the improved model could not be installed");
            return Ok(legacy);
        }
        return Err(error);
    }
    tracing::info!(
        model = MODEL_NAME,
        "local assistant model installed and verified"
    );
    Ok(destination)
}

async fn verify_model(path: PathBuf) -> Result<bool> {
    verify_model_checksum(path, MODEL_SHA256, MAX_MODEL_BYTES).await
}

async fn verify_model_checksum(path: PathBuf, expected: &str, max_bytes: u64) -> Result<bool> {
    let expected = expected.to_owned();
    tokio::task::spawn_blocking(move || {
        if !path.is_file() {
            return Ok(false);
        }
        let mut file = BufReader::new(File::open(&path)?);
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 128 * 1024];
        let mut total = 0_u64;
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > max_bytes {
                return Ok(false);
            }
            hasher.update(&buffer[..read]);
        }
        Ok(total > 0 && hex::encode(hasher.finalize()) == expected)
    })
    .await
    .context("assistant model verification worker stopped")?
}

async fn download_model(path: &Path) -> Result<()> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30 * 60))
        .user_agent("Frokla-local-assistant/1.0")
        .build()?;
    let mut response = client
        .get(MODEL_URL)
        .send()
        .await
        .context("download model from the official Hugging Face Qwen repository")?
        .error_for_status()
        .context("model source returned an error")?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MODEL_BYTES)
    {
        bail!("model download exceeds the 1250 MiB safety limit");
    }

    let mut file = fs::File::create(path)
        .await
        .with_context(|| format!("create partial model file {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let mut next_log = 64 * 1024 * 1024;
    while let Some(chunk) = response.chunk().await.context("download model chunk")? {
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > MAX_MODEL_BYTES {
            bail!("model download exceeds the 1250 MiB safety limit");
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .context("write model download")?;
        if downloaded >= next_log {
            tracing::info!(
                downloaded_mib = downloaded / (1024 * 1024),
                "assistant model download progress"
            );
            next_log += 64 * 1024 * 1024;
        }
    }
    file.sync_all()
        .await
        .context("flush model download to disk")?;
    let actual = hex::encode(hasher.finalize());
    if actual != MODEL_SHA256 {
        bail!("assistant model checksum mismatch; expected {MODEL_SHA256}, received {actual}");
    }
    Ok(())
}

pub async fn start_worker(path: PathBuf) -> Result<AssistantService> {
    let (tx, mut rx) = mpsc::channel::<Job>(1);
    let (ready_tx, ready_rx) = oneshot::channel::<std::result::Result<(), String>>();
    thread::Builder::new()
        .name("frokla-local-assistant".into())
        .spawn(move || {
            let backend = match LlamaBackend::init() {
                Ok(backend) => backend,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("initialize local model runtime: {error}")));
                    return;
                }
            };
            let params = LlamaModelParams::default()
                .with_n_gpu_layers(0)
                .with_use_mmap(true);
            let model = match LlamaModel::load_from_file(&backend, &path, &params) {
                Ok(model) => model,
                Err(error) => {
                    let _ = ready_tx.send(Err(format!("load local assistant model: {error}")));
                    return;
                }
            };
            if ready_tx.send(Ok(())).is_err() {
                return;
            }
            while let Some(job) = rx.blocking_recv() {
                let answer = generate(&backend, &model, &job.prompt);
                let _ = job.answer.send(answer);
            }
        })
        .context("start local assistant worker thread")?;

    match tokio::time::timeout(Duration::from_secs(120), ready_rx).await {
        Ok(Ok(Ok(()))) => Ok(AssistantService { tx }),
        Ok(Ok(Err(error))) => bail!("{error}"),
        Ok(Err(_)) => bail!("local assistant worker stopped during startup"),
        Err(_) => bail!("local assistant model did not load within two minutes"),
    }
}

fn generate(backend: &LlamaBackend, model: &LlamaModel, user_prompt: &str) -> AssistantReply {
    if is_likely_gibberish(user_prompt) {
        return Ok("Не разобрал пожелание. Напиши, кому адресовать открытку и каким сделать текст — тёплым, весёлым или коротким 🎄".into());
    }
    if !is_card_or_new_year_request(user_prompt) {
        return Ok("Я помогаю с текстами открыток и новогодними поздравлениями 🎄 Напиши, для кого будет текст и какое настроение выбрать ✨".into());
    }
    let compact_request = user_prompt
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    let actionable_prompt = match compact_request.as_str() {
        "открытку" | "открытка" | "новогоднююоткрытку" => {
            "Составь один короткий, тёплый текст новогодней открытки на русском языке: два предложения, готовых к отправке. Если адресат не указан, начни словами «Дорогой Фрокла». Добавь 1–2 уместных эмодзи. Выведи только текст открытки."
        }
        "поздравление" | "пожелание" | "новыйгод" | "сновымгодом" => {
            "Напиши одно короткое, тёплое новогоднее поздравление на русском языке в двух предложениях, с 1–2 уместными эмодзи. Если адресат не указан, начни словами «Дорогой Фрокла». Выведи только готовый текст, без списка и вступления."
        }
        _ => user_prompt,
    };
    let prompt = format!(
        "<|im_start|>system\nТы — помощник Frokla: пиши только готовые русские открытки и поздравления. На запрос вне этой темы ответь, что помогаешь только с открытками и Новым годом 🎄. Если повод для открытки не указан, выбери Новый год. Выводи только текст, без заголовка и объяснений. Соблюдай адресата, родство, пол, возраст, повод, тон и длину запроса; не выдумывай факты и не меняй родство. Внук — мужской род, внучка — женский; указанный возраст обязательно упомяни. Без адресата начни «Дорогой Фрокла» в мужском роде. Если просят N строк, выведи ровно N строк без нумерации. Пиши кратко, естественно, добавь 1–3 уместных эмодзи.<|im_end|>\n<|im_start|>user\nКороткая новогодняя открытка без указанного адресата.<|im_end|>\n<|im_start|>assistant\nДорогой Фрокла, с Новым годом! Пусть рядом будут близкие, а впереди ждут радость и хорошие перемены 🎄✨<|im_end|>\n<|im_start|>user\nОткрытка на Новый год внуку, ему 21 год.<|im_end|>\n<|im_start|>assistant\nДорогой внук, с Новым годом! В 21 год желаю смелых идей, ярких впечатлений и удачи 🎄✨<|im_end|>\n<|im_start|>user\n{actionable_prompt}<|im_end|>\n<|im_start|>assistant\n"
    );
    let tokens = model.vocab().tokenize(prompt.as_bytes(), true, true);
    if tokens.is_empty() {
        return Err(
            "Модель не смогла разобрать запрос. Попробуй сформулировать его короче.".into(),
        );
    }
    if tokens.len() > 512 || tokens.len() + MAX_GENERATED_TOKENS >= CONTEXT_TOKENS as usize {
        return Err("Запрос слишком длинный для локальной модели.".into());
    }

    let context_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(CONTEXT_TOKENS))
        .with_n_batch(512)
        .with_n_threads(2)
        .with_n_threads_batch(2);
    let mut context = model
        .new_context(backend, context_params)
        .map_err(|error| format!("Не удалось создать контекст модели: {error}"))?;
    let mut batch = LlamaBatch::new(tokens.len() + 1, 1);
    batch
        .add_sequence(&tokens, 0, false)
        .map_err(|error| format!("Не удалось подготовить запрос к модели: {error}"))?;
    context
        .decode(&mut batch)
        .map_err(|error| format!("Не удалось обработать запрос моделью: {error}"))?;

    let mut sampler = LlamaSampler::chain(
        [
            LlamaSampler::temp(0.4),
            LlamaSampler::top_p(0.85, 1),
            LlamaSampler::dist(rand::random()),
        ],
        true,
    );
    let mut n_cur = batch.n_tokens();
    let mut output = Vec::with_capacity(1024);
    for _ in 0..MAX_GENERATED_TOKENS {
        let token = sampler.sample(&context, batch.n_tokens() - 1);
        sampler.accept(token);
        if model.vocab().is_eog(token) {
            break;
        }
        output.extend(model.vocab().token_to_piece(token, false, None));
        if n_cur as u32 >= CONTEXT_TOKENS - 1 {
            break;
        }
        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|error| format!("Не удалось продолжить генерацию: {error}"))?;
        n_cur += 1;
        context
            .decode(&mut batch)
            .map_err(|error| format!("Генерация ответа прервалась: {error}"))?;
    }
    let answer = String::from_utf8_lossy(&output)
        .replace("<|im_end|>", "")
        .replace("<|im_start|>", "")
        .trim()
        .to_owned();
    if answer.is_empty() {
        return Err("Модель не выдала ответ. Попробуй ещё раз.".into());
    }
    if is_ready_to_send(&answer, user_prompt) {
        if requested_line_count(user_prompt).is_some() {
            Ok(answer
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("\n"))
        } else {
            Ok(answer.split_whitespace().collect::<Vec<_>>().join(" "))
        }
    } else {
        Ok(fallback_card_text(user_prompt))
    }
}

fn is_ready_to_send(answer: &str, input: &str) -> bool {
    let trimmed = answer.trim();
    let lower = trimmed.to_lowercase();
    let word_count = trimmed.split_whitespace().count();
    let requested_lines = requested_line_count(input);
    let output_lines = trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let max_words = requested_lines.map_or(40, |count| count * 10);
    let line_count_matches =
        requested_lines.map_or(output_lines <= 1, |count| output_lines == count);
    let sentence_count = trimmed
        .chars()
        .filter(|character| matches!(character, '.' | '!' | '?'))
        .count();
    let has_emoji = [
        "🎄", "✨", "🎁", "❄", "🥂", "💛", "❤️", "🎉", "🌟", "🧡", "💫", "⭐", "☃", "😊",
    ]
    .iter()
    .any(|emoji| trimmed.contains(emoji));
    let has_cyrillic = trimmed
        .chars()
        .any(|character| matches!(character, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
    let has_unexpected_script = trimmed.chars().any(|character| {
        character.is_alphabetic()
            && !character.is_ascii_alphabetic()
            && !matches!(character, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
    });
    let has_preamble = [
        "вот открытка",
        "вот поздравление",
        "вот вариант",
        "конечно",
        "держи",
        "текст открытки:",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));
    let has_list = trimmed.lines().any(|line| {
        let line = line.trim_start();
        if line.starts_with('-') || line.starts_with('•') || line.starts_with('*') {
            return true;
        }
        let digits = line
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        digits > 0
            && line
                .get(digits..)
                .unwrap_or_default()
                .trim_start()
                .chars()
                .next()
                .is_some_and(|character| matches!(character, '.' | ')'))
    });

    let sentence_limit = requested_lines.map_or(3, |count| count + 1);
    let recipient_is_unspecified = recipient_is_unspecified(input);
    let has_default_recipient = lower.starts_with("дорогой фрокла");
    let recipient_matches = expected_recipient_term(input).is_none_or(|term| lower.contains(term));
    let requested_age = requested_age(input);
    let age_matches = requested_age.is_none_or(|age| lower.contains(&age.to_string()));

    (4..=max_words).contains(&word_count)
        && line_count_matches
        && (1..=sentence_limit).contains(&sentence_count)
        && has_emoji
        && has_cyrillic
        && !has_unexpected_script
        && !has_preamble
        && !has_list
        && recipient_matches
        && age_matches
        && (!recipient_is_unspecified || has_default_recipient)
}

fn fallback_card_text(input: &str) -> String {
    let normalized = input.to_lowercase().replace('ё', "е");
    let humorous =
        normalized.contains("юмор") || normalized.contains("смешн") || normalized.contains("весел");

    if let Some(count) = requested_line_count(input) {
        let mut greeting = if normalized.contains("день рождения") {
            recipient_greeting(&normalized)
                .map(|name| format!("{name}, с днём рождения! 🎂"))
                .unwrap_or_else(|| "С днём рождения! 🎂".to_owned())
        } else if normalized.contains("8 марта") {
            recipient_greeting(&normalized)
                .map(|name| format!("{name}, с 8 Марта! 🌷"))
                .unwrap_or_else(|| "С 8 Марта! 🌷".to_owned())
        } else if let Some(greeting) = new_year_greeting(&normalized) {
            greeting.to_owned()
        } else {
            "С Новым годом! 🎄".to_owned()
        };
        let wishes = if humorous {
            [
                "Пусть оливье не кончается.",
                "А будильник в праздники молчит.",
                "Пусть планы сбываются легко.",
                "И поводов для смеха будет больше.",
                "Пусть рядом будут самые близкие.",
                "Желаю удачи во всех делах.",
                "Пусть сюрпризы будут только приятными.",
                "А заботы подождут до января.",
                "Пусть каждый день радует! ✨",
            ]
        } else {
            [
                "Пусть впереди будет много радостных дней.",
                "Пусть рядом всегда будут близкие люди.",
                "Желаю уюта, тепла и вдохновения.",
                "Пусть сбываются самые добрые мечты.",
                "Пусть каждый день приносит хорошие новости.",
                "Желаю сил для всего задуманного.",
                "Пусть дома всегда ждут смех и уют.",
                "Пусть впереди будут приятные сюрпризы.",
                "И будет больше поводов улыбаться! ✨",
            ]
        };
        let mut lines = Vec::with_capacity(count);
        let age = requested_age(input);
        if count == 1 {
            if let Some(age) = age {
                greeting.push(' ');
                greeting.push_str(&age_wish(age));
            }
        }
        lines.push(greeting);
        let mut remaining = count.saturating_sub(1);
        if remaining > 0 {
            if let Some(age) = age {
                lines.push(age_wish(age));
                remaining -= 1;
            }
        }
        lines.extend(wishes.into_iter().take(remaining).map(str::to_owned));
        return lines.join("\n");
    }

    if let Some(text) = recipient_fallback_text(input, &normalized, humorous) {
        return text;
    }

    if normalized.contains("день рождения") {
        let greeting = if recipient_is_unspecified(&normalized) || normalized.contains("фрокла")
        {
            "Дорогой Фрокла, с днём рождения!"
        } else {
            "С днём рождения!"
        };
        return format!("{greeting} Пусть впереди будет много радостных встреч, добрых новостей и поводов гордиться собой 🎂✨");
    }
    if normalized.contains("8 марта") {
        let greeting = if recipient_is_unspecified(&normalized) || normalized.contains("фрокла")
        {
            "Дорогой Фрокла, с 8 Марта!"
        } else {
            "С 8 Марта!"
        };
        return format!("{greeting} Желаю тепла, весеннего настроения и побольше приятных моментов рядом с любимыми людьми 🌷✨");
    }
    if normalized.contains("23 февраля") {
        let greeting = if recipient_is_unspecified(&normalized) || normalized.contains("фрокла")
        {
            "Дорогой Фрокла, с 23 Февраля!"
        } else {
            "С праздником!"
        };
        return format!("{greeting} Желаю сил, уверенности, надёжных людей рядом и побольше хороших новостей 🎁✨");
    }
    if humorous {
        return "Дорогой Фрокла, с Новым годом! Пусть оливье не кончается, будильник в праздники молчит, а удача идёт рядом 🎄🥂".into();
    }

    "Дорогой Фрокла, с Новым годом! Пусть в доме будет тепло, рядом — любимые люди, а впереди ждут радостные встречи и добрые перемены 🎄✨".into()
}

fn recipient_is_unspecified(normalized_input: &str) -> bool {
    expected_recipient_term(normalized_input).is_none()
}

fn expected_recipient_term(normalized_input: &str) -> Option<&'static str> {
    let normalized_input = normalized_input.to_lowercase().replace('ё', "е");
    [
        ("внуч", "внуч"),
        ("внук", "внук"),
        ("доч", "доч"),
        ("сын", "сын"),
        ("мам", "мам"),
        ("пап", "пап"),
        ("бабуш", "бабуш"),
        ("дедуш", "дедуш"),
        ("подруг", "подруг"),
        ("друг", "друг"),
        ("девушк", "девушк"),
        ("парн", "парн"),
        ("коллег", "коллег"),
        ("брат", "брат"),
        ("сестр", "сестр"),
        ("жен", "жен"),
        ("муж", "муж"),
        ("фрокла", "фрокла"),
    ]
    .iter()
    .find_map(|(needle, expected)| normalized_input.contains(needle).then_some(*expected))
}

fn recipient_greeting(normalized_input: &str) -> Option<&'static str> {
    match expected_recipient_term(normalized_input)? {
        "внук" => Some("Дорогой внук"),
        "внуч" => Some("Дорогая внучка"),
        "сын" => Some("Дорогой сын"),
        "доч" => Some("Дорогая дочка"),
        "мам" => Some("Мамочка"),
        "пап" => Some("Папа"),
        "бабуш" => Some("Дорогая бабушка"),
        "дедуш" => Some("Дорогой дедушка"),
        "подруг" => Some("Дорогая подруга"),
        "друг" => Some("Дорогой друг"),
        "девушк" => Some("Дорогая"),
        "парн" => Some("Дорогой"),
        "коллег" => Some("Дорогой коллега"),
        "брат" => Some("Дорогой брат"),
        "сестр" => Some("Дорогая сестра"),
        "жен" => Some("Дорогая"),
        "муж" => Some("Дорогой"),
        "фрокла" => Some("Дорогой Фрокла"),
        _ => None,
    }
}

fn new_year_greeting(normalized_input: &str) -> Option<&'static str> {
    let greeting = recipient_greeting(normalized_input)?;
    Some(match greeting {
        "Мамочка" => "Мамочка, с Новым годом! 🎄",
        "Папа" => "Папа, с Новым годом! 🎄",
        "Дорогой внук" => "Дорогой внук, с Новым годом! 🎄",
        "Дорогая внучка" => "Дорогая внучка, с Новым годом! 🎄",
        "Дорогой сын" => "Дорогой сын, с Новым годом! 🎄",
        "Дорогая дочка" => "Дорогая дочка, с Новым годом! 🎄",
        "Дорогая бабушка" => "Дорогая бабушка, с Новым годом! 🎄",
        "Дорогой дедушка" => "Дорогой дедушка, с Новым годом! 🎄",
        "Дорогая подруга" => "Дорогая подруга, с Новым годом! 🎄",
        "Дорогой друг" => "Дорогой друг, с Новым годом! 🎄",
        "Дорогая сестра" => "Дорогая сестра, с Новым годом! 🎄",
        "Дорогой брат" => "Дорогой брат, с Новым годом! 🎄",
        "Дорогой коллега" => "Дорогой коллега, с Новым годом! 🎄",
        "Дорогой Фрокла" => "Дорогой Фрокла, с Новым годом! 🎄",
        "Дорогая" => "Дорогая, с Новым годом! 🎄",
        "Дорогой" => "Дорогой, с Новым годом! 🎄",
        _ => return None,
    })
}

fn recipient_fallback_text(input: &str, normalized: &str, humorous: bool) -> Option<String> {
    let greeting = recipient_greeting(normalized)?;
    let (occasion, emoji) = if normalized.contains("день рождения") {
        ("с днём рождения", "🎂✨")
    } else if normalized.contains("8 марта") {
        ("с 8 Марта", "🌷✨")
    } else if normalized.contains("23 февраля") {
        ("с 23 Февраля", "🎁✨")
    } else {
        ("с Новым годом", "🎄✨")
    };
    let age_text = requested_age(input).map(|age| age_wish(age));
    let wish = if let Some(age_text) = age_text {
        format!("{age_text} Пусть впереди будут яркие впечатления, хорошие друзья и успех в задуманном {emoji}")
    } else if humorous {
        format!("Пусть планы сбываются, поводов для смеха будет больше, а будильник в праздники помалкивает {emoji}")
    } else {
        format!("Желаю тебе радостных встреч, интересных планов и удачи во всём задуманном {emoji}")
    };
    Some(format!("{greeting}, {occasion}! {wish}"))
}

fn requested_age(input: &str) -> Option<u8> {
    let words = input.split_whitespace().collect::<Vec<_>>();
    words.windows(2).find_map(|pair| {
        let number = pair[0].trim_matches(|character: char| !character.is_ascii_digit());
        let age = number.parse::<u8>().ok()?;
        let unit = pair[1].trim_matches(|character: char| !character.is_alphabetic());
        ((3..=120).contains(&age) && (unit.starts_with("год") || unit.starts_with("лет")))
            .then_some(age)
    })
}

fn age_wish(age: u8) -> String {
    format!(
        "В {age} {} желаю смело идти к своим целям и открывать новые возможности.",
        age_unit(age)
    )
}

fn age_unit(age: u8) -> &'static str {
    let last_two = age % 100;
    if (11..=14).contains(&last_two) {
        "лет"
    } else {
        match age % 10 {
            1 => "год",
            2..=4 => "года",
            _ => "лет",
        }
    }
}

fn requested_line_count(input: &str) -> Option<usize> {
    let words = input
        .to_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for (index, word) in words.iter().enumerate() {
        if !(word.starts_with("строк") || word.starts_with("строч")) || index == 0 {
            continue;
        }
        let previous = words[index - 1].trim_matches(|character: char| !character.is_ascii_digit());
        if let Ok(count) = previous.parse::<usize>() {
            return (1..=10).contains(&count).then_some(count);
        }
        let count = match previous {
            "одну" | "одна" | "один" => 1,
            "две" | "два" => 2,
            "три" => 3,
            "четыре" => 4,
            "пять" => 5,
            "шесть" => 6,
            "семь" => 7,
            "восемь" => 8,
            "девять" => 9,
            "десять" => 10,
            _ => return None,
        };
        return Some(count);
    }
    None
}

fn is_likely_gibberish(input: &str) -> bool {
    let letters = input
        .chars()
        .filter(|character| character.is_alphabetic())
        .map(|character| character.to_lowercase().next().unwrap_or(character))
        .collect::<Vec<_>>();
    if letters.len() < 8 {
        return false;
    }

    let unique = letters.iter().collect::<std::collections::HashSet<_>>();
    if unique.len() <= 3 {
        return true;
    }

    if letters.len() >= 10 {
        let vowel_count = letters
            .iter()
            .filter(|character| "аеёиоуыэюяaeiouy".contains(**character))
            .count();
        return vowel_count <= 1;
    }
    false
}

fn is_card_or_new_year_request(input: &str) -> bool {
    let normalized = input.to_lowercase().replace('ё', "е");
    let postcard = normalized.contains("открытк");
    let new_year = [
        "новогод",
        "новый год",
        "нового года",
        "с новым годом",
        "елк",
        "дед мороз",
        "снегуроч",
        "курант",
    ]
    .iter()
    .any(|term| normalized.contains(term));
    let other_occasion = [
        "день рождения",
        "8 марта",
        "23 февраля",
        "14 февраля",
        "день учителя",
        "день победы",
        "свадьб",
        "юбиле",
    ]
    .iter()
    .any(|term| normalized.contains(term));

    if other_occasion && !postcard && !new_year {
        return false;
    }

    postcard || new_year || normalized.contains("поздрав") || normalized.contains("пожелан")
}
