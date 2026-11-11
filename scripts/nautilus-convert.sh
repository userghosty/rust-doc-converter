#!/usr/bin/env bash
# Nautilus передаёт выбранные локальные файлы в аргументах.
set -u
converter="${HOME}/.local/bin/doc-converter"
if (( $# != 1 )); then
    zenity --error --title='Конвертер документов' --text='Выберите один файл DOCX или PDF.'
    exit 1
fi
source_file=$(realpath -- "$1") || exit 1
case "${source_file,,}" in
    *.docx) extension=pdf ;;
    *.pdf)  extension=docx ;;
    *) zenity --error --title='Конвертер документов' --text='Поддерживаются только файлы DOCX и PDF.'; exit 1 ;;
esac
if [[ ! -x "$converter" ]]; then
    zenity --error --title='Конвертер документов' --text='Приложение не найдено. Возможно, папка проекта была перемещена.'
    exit 1
fi
suggested="${source_file%.*}.${extension}"
if ! output_file=$(zenity --file-selection --save --title='Куда сохранить документ?' --filename="$suggested"); then
    exit 0
fi
[[ -n "$output_file" ]] || exit 0
command -v notify-send >/dev/null && notify-send 'Конвертер документов' 'Конвертация началась…'
if result=$("$converter" --convert "$source_file" "$output_file" 2>&1); then
    result=${result##*$'\n'}
    zenity --info --title='Конвертер документов' --text="Готово! Файл сохранён:
$result"
else
    message=${result##*$'\n'}
    zenity --error --title='Конвертер документов' --text="Не удалось преобразовать файл:
$message"
    exit 1
fi
