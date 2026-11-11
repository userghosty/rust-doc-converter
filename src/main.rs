use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    ptr,
    sync::mpsc,
    thread,
};
type Ptr = *mut c_void;
#[link(name = "gtk-4")]
extern "C" {
    fn gtk_application_new(id: *const c_char, flags: u32) -> Ptr;
    fn gtk_application_window_new(app: Ptr) -> Ptr;
    fn gtk_window_set_title(w: Ptr, s: *const c_char);
    fn gtk_window_set_default_size(w: Ptr, x: c_int, y: c_int);
    fn gtk_window_set_child(w: Ptr, child: Ptr);
    fn gtk_window_present(w: Ptr);
    fn gtk_box_new(orientation: c_int, spacing: c_int) -> Ptr;
    fn gtk_box_append(b: Ptr, child: Ptr);
    fn gtk_widget_set_margin_top(w: Ptr, n: c_int);
    fn gtk_widget_set_margin_bottom(w: Ptr, n: c_int);
    fn gtk_widget_set_margin_start(w: Ptr, n: c_int);
    fn gtk_widget_set_margin_end(w: Ptr, n: c_int);
    fn gtk_widget_set_sensitive(w: Ptr, v: c_int);
    fn gtk_widget_set_halign(w: Ptr, align: c_int);
    fn gtk_widget_set_valign(w: Ptr, align: c_int);
    fn gtk_widget_set_size_request(w: Ptr, width: c_int, height: c_int);
    fn gtk_widget_add_css_class(w: Ptr, name: *const c_char);
    fn gtk_label_set_xalign(l: Ptr, x: f32);
    fn gtk_css_provider_new() -> Ptr;
    fn gtk_css_provider_load_from_data(provider: Ptr, data: *const c_char, length: isize);
    fn gtk_style_context_add_provider_for_display(display: Ptr, provider: Ptr, priority: u32);
    fn gdk_display_get_default() -> Ptr;
    fn gtk_file_chooser_set_current_name(chooser: Ptr, name: *const c_char);
    fn gtk_file_chooser_set_current_folder(chooser: Ptr, file: Ptr, error: *mut Ptr) -> c_int;
    fn gtk_label_new(s: *const c_char) -> Ptr;
    fn gtk_label_set_text(l: Ptr, s: *const c_char);
    fn gtk_label_set_wrap(l: Ptr, v: c_int);
    fn gtk_button_new_with_label(s: *const c_char) -> Ptr;
    fn gtk_file_chooser_native_new(
        title: *const c_char,
        parent: Ptr,
        action: c_int,
        accept: *const c_char,
        cancel: *const c_char,
    ) -> Ptr;
    fn gtk_native_dialog_show(dialog: Ptr);
    fn gtk_native_dialog_destroy(dialog: Ptr);
    fn gtk_file_chooser_get_file(chooser: Ptr) -> Ptr;
}
#[link(name = "gio-2.0")]
extern "C" {
    fn g_application_run(app: Ptr, argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn g_file_get_path(file: Ptr) -> *mut c_char;
    fn g_file_new_for_path(path: *const c_char) -> Ptr;
}
#[link(name = "gobject-2.0")]
extern "C" {
    fn g_signal_connect_data(
        instance: Ptr,
        signal: *const c_char,
        callback: Ptr,
        data: Ptr,
        destroy: Ptr,
        flags: u32,
    ) -> u64;
    fn g_object_unref(obj: Ptr);
}
#[link(name = "glib-2.0")]
extern "C" {
    fn g_free(p: Ptr);
    fn g_timeout_add(interval: u32, func: extern "C" fn(Ptr) -> c_int, data: Ptr) -> u32;
}
fn c(s: &str) -> CString {
    CString::new(s.replace('\0', "�")).unwrap()
}
struct State {
    window: Ptr,
    label: Ptr,
    button: Ptr,
    source: Option<PathBuf>,
}
unsafe fn set_label(label: Ptr, text: &str) {
    gtk_label_set_text(label, c(text).as_ptr())
}
unsafe fn signal(obj: Ptr, name: &str, cb: Ptr, data: Ptr) {
    g_signal_connect_data(obj, c(name).as_ptr(), cb, data, ptr::null_mut(), 0);
}

fn output_extension(source: &Path) -> Result<&'static str, String> {
    match source
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "docx" => Ok("pdf"),
        "pdf" => Ok("docx"),
        _ => Err("Выберите файл DOCX или PDF.".into()),
    }
}

fn ensure_extension(mut path: PathBuf, ext: &str) -> PathBuf {
    if !path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case(ext))
    {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{ext}"));
        path.set_file_name(name);
    }
    path
}

fn convert(source: &Path, target: &Path) -> Result<PathBuf, String> {
    let ext = output_extension(source)?;
    let target = ensure_extension(target.to_path_buf(), ext);
    if target.exists() {
        return Err("Файл с таким именем уже существует. Выберите другое имя.".into());
    }
    let size = fs::metadata(source).map_err(|e| e.to_string())?.len();
    if size == 0 || size > 50 * 1024 * 1024 {
        return Err("Размер файла должен быть от 1 байта до 50 МБ.".into());
    }
    let bytes = if ext == "pdf" {
        let input = fs::read(source).map_err(|e| e.to_string())?;
        dxpdf::convert(&input).map_err(|e| e.to_string())?
    } else {
        let doc = pdf_oxide::PdfDocument::open(source).map_err(|e| e.to_string())?;
        let pages = doc.page_count().map_err(|e| e.to_string())?;
        let mut has_text = false;
        for i in 0..pages {
            if !doc
                .extract_text(i)
                .map_err(|e| e.to_string())?
                .trim()
                .is_empty()
            {
                has_text = true;
                break;
            }
        }
        if !has_text {
            return Err(
                "В PDF нет распознаваемого текста. Конвертация сканов пока не поддерживается."
                    .into(),
            );
        }
        doc.to_docx_bytes().map_err(|e| e.to_string())?
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                "Файл с таким именем уже существует. Выберите другое имя.".to_string()
            } else {
                e.to_string()
            }
        })?;
    if let Err(e) = file.write_all(&bytes) {
        drop(file);
        let _ = fs::remove_file(&target);
        return Err(e.to_string());
    }
    Ok(target)
}
struct Pending {
    state: *mut State,
    rx: mpsc::Receiver<Result<PathBuf, String>>,
}
extern "C" fn poll(data: Ptr) -> c_int {
    unsafe {
        let p = &mut *(data as *mut Pending);
        match p.rx.try_recv() {
            Ok(result) => {
                let state = &*p.state;
                gtk_widget_set_sensitive(state.button, 1);
                match result {
                    Ok(path) => set_label(
                        state.label,
                        &format!("Готово! Файл сохранён:\n{}", path.display()),
                    ),
                    Err(e) => {
                        set_label(state.label, &format!("Не удалось преобразовать файл:\n{e}"))
                    }
                }
                drop(Box::from_raw(data as *mut Pending));
                0
            }
            Err(mpsc::TryRecvError::Empty) => 1,
            Err(_) => {
                let state = &*p.state;
                gtk_widget_set_sensitive(state.button, 1);
                set_label(state.label, "Ошибка конвертации.");
                drop(Box::from_raw(data as *mut Pending));
                0
            }
        }
    }
}
unsafe fn selected_path(dialog: Ptr) -> Option<PathBuf> {
    let file = gtk_file_chooser_get_file(dialog);
    if file.is_null() {
        return None;
    }
    let raw = g_file_get_path(file);
    let result = if raw.is_null() {
        None
    } else {
        let path = PathBuf::from(CStr::from_ptr(raw).to_string_lossy().into_owned());
        g_free(raw as Ptr);
        Some(path)
    };
    g_object_unref(file);
    result
}
extern "C" fn save_chosen(dialog: Ptr, response: c_int, data: Ptr) {
    unsafe {
        let state = &mut *(data as *mut State);
        if response == -3 {
            if let (Some(target), Some(source)) = (selected_path(dialog), state.source.take()) {
                gtk_widget_set_sensitive(state.button, 0);
                set_label(state.label, "Конвертируем документ…");
                let (tx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let _ = tx.send(convert(&source, &target));
                });
                let pending = Box::new(Pending {
                    state: data as *mut State,
                    rx,
                });
                g_timeout_add(100, poll, Box::into_raw(pending) as Ptr);
            }
        } else {
            state.source = None;
            set_label(
                state.label,
                "Сохранение отменено. Выберите документ заново.",
            );
        }
        gtk_native_dialog_destroy(dialog);
        g_object_unref(dialog);
    }
}
extern "C" fn chosen(dialog: Ptr, response: c_int, data: Ptr) {
    unsafe {
        let state = &mut *(data as *mut State);
        if response == -3 {
            if let Some(source) = selected_path(dialog) {
                match output_extension(&source) {
                    Ok(ext) => {
                        let suggestion = source.with_extension(ext);
                        let save = gtk_file_chooser_native_new(
                            c("Куда сохранить документ?").as_ptr(),
                            state.window,
                            1,
                            c("Сохранить").as_ptr(),
                            c("Отмена").as_ptr(),
                        );
                        if let Some(name) = suggestion.file_name() {
                            gtk_file_chooser_set_current_name(
                                save,
                                c(&name.to_string_lossy()).as_ptr(),
                            );
                        }
                        if let Some(parent) = source.parent() {
                            let folder = g_file_new_for_path(c(&parent.to_string_lossy()).as_ptr());
                            gtk_file_chooser_set_current_folder(save, folder, ptr::null_mut());
                            g_object_unref(folder);
                        }
                        state.source = Some(source);
                        set_label(state.label, "Выберите папку и имя для результата.");
                        signal(save, "response", save_chosen as Ptr, data);
                        gtk_native_dialog_show(save);
                    }
                    Err(e) => set_label(state.label, &e),
                }
            }
        }
        gtk_native_dialog_destroy(dialog);
        g_object_unref(dialog);
    }
}
extern "C" fn choose(_button: Ptr, data: Ptr) {
    unsafe {
        let s = &*(data as *mut State);
        let dlg = gtk_file_chooser_native_new(
            c("Выберите DOCX или PDF").as_ptr(),
            s.window,
            0,
            c("Открыть").as_ptr(),
            c("Отмена").as_ptr(),
        );
        signal(dlg, "response", chosen as Ptr, data);
        gtk_native_dialog_show(dlg);
    }
}
unsafe fn add_class(widget: Ptr, name: &str) {
    gtk_widget_add_css_class(widget, c(name).as_ptr());
}
extern "C" fn activate(app: Ptr, _data: Ptr) {
    unsafe {
        let css = r#"
            window { background: #101723; color: #eff3f8; }
            .card { background: #1b2736; border: 1px solid #36475b; border-radius: 24px; padding: 32px; }
            .eyebrow { color: #8dbcfb; font-size: 12px; font-weight: 700; letter-spacing: 2px; }
            .title { color: #f2f6ff; font-size: 30px; font-weight: 750; }
            .subtitle { color: #b9c8d9; font-size: 15px; }
            .convert-button { background: #91bfff; color: #102137; border-radius: 12px; border: none; min-height: 52px; font-size: 15px; font-weight: 700; box-shadow: none; }
            .convert-button:hover { background: #b2d2ff; }
            .status { color: #c8d5e4; font-size: 13px; }
        "#;
        let provider = gtk_css_provider_new();
        gtk_css_provider_load_from_data(provider, c(css).as_ptr(), -1);
        gtk_style_context_add_provider_for_display(gdk_display_get_default(), provider, 600);
        g_object_unref(provider);
        let window = gtk_application_window_new(app);
        gtk_window_set_title(window, c("Конвертер документов").as_ptr());
        gtk_window_set_default_size(window, 620, 470);
        let layout = gtk_box_new(1, 17);
        gtk_widget_set_halign(layout, 3);
        gtk_widget_set_valign(layout, 3);
        gtk_widget_set_size_request(layout, 420, -1);
        for set in [
            gtk_widget_set_margin_top,
            gtk_widget_set_margin_bottom,
            gtk_widget_set_margin_start,
            gtk_widget_set_margin_end,
        ] {
            set(layout, 24)
        }
        add_class(layout, "card");
        let badge = gtk_label_new(c("DOCX  ↔  PDF").as_ptr());
        gtk_label_set_xalign(badge, 0.0);
        add_class(badge, "eyebrow");
        gtk_box_append(layout, badge);
        let title = gtk_label_new(c("Конвертер документов").as_ptr());
        gtk_label_set_xalign(title, 0.0);
        add_class(title, "title");
        gtk_box_append(layout, title);
        let description =
            gtk_label_new(c("Выберите документ и укажите, куда сохранить готовый файл.").as_ptr());
        gtk_label_set_xalign(description, 0.0);
        gtk_label_set_wrap(description, 1);
        add_class(description, "subtitle");
        gtk_box_append(layout, description);
        let button = gtk_button_new_with_label(c("Выбрать документ").as_ptr());
        add_class(button, "convert-button");
        gtk_box_append(layout, button);
        let label =
            gtk_label_new(c("Документы обрабатываются локально на вашем компьютере.").as_ptr());
        gtk_label_set_xalign(label, 0.0);
        gtk_label_set_wrap(label, 1);
        add_class(label, "status");
        gtk_box_append(layout, label);
        gtk_window_set_child(window, layout);
        let state = Box::into_raw(Box::new(State {
            window,
            label,
            button,
            source: None,
        }));
        signal(button, "clicked", choose as Ptr, state as Ptr);
        gtk_window_present(window);
    }
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 4 && args[1] == "--convert" {
        match convert(Path::new(&args[2]), Path::new(&args[3])) {
            Ok(p) => println!("{}", p.display()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1)
            }
        }
        return;
    }
    unsafe {
        let app = gtk_application_new(c("ru.local.docconverter").as_ptr(), 0);
        signal(app, "activate", activate as Ptr, ptr::null_mut());
        let code = g_application_run(app, 0, ptr::null_mut());
        g_object_unref(app);
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_extension_matches_source_format() {
        assert_eq!(output_extension(Path::new("test.docx")).unwrap(), "pdf");
        assert_eq!(output_extension(Path::new("Тест.PDF")).unwrap(), "docx");
        assert!(output_extension(Path::new("test.txt")).is_err());
    }
    #[test]
    fn adds_extension_when_missing() {
        assert_eq!(
            ensure_extension(PathBuf::from("/tmp/отчет"), "pdf"),
            PathBuf::from("/tmp/отчет.pdf")
        );
        assert_eq!(
            ensure_extension(PathBuf::from("/tmp/отчет.PDF"), "pdf"),
            PathBuf::from("/tmp/отчет.PDF")
        );
    }
}
