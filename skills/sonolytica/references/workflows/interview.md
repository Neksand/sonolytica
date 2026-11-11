# Workflow: interview / podcast / 1:1

Goal: who said what, where, and what the episode is about — with quotes.

## Commands, in order

```bash
sonolytica doctor
sonolytica analyze interview.mp3 --out interview.json --profile standard --summary
sonolytica query brief interview.json        # status, speakers, chapters, counts
# only if you need the full lists:
sonolytica query speakers interview.json
sonolytica query chapters interview.json
```

Then answer the question:

```bash
sonolytica query timeline interview.json --from 00:12:30 --to 00:14:00
sonolytica query search interview.json pricing
sonolytica query search interview.json deadline
sonolytica query transcript interview.json        # full text, untruncated
sonolytica query words interview.json 4           # one utterance, word by word
```

## Answer shape

1. **Состояние**: одной строкой — все стадии `complete`, или что именно
   отсутствует (`partial`, `provider_unavailable`).
2. **Кто говорил**: `query speakers`, с локальными метками и суммарным временем.
3. **Структура**: главы из `query chapters` — диапазон, спикеры, `title_hint`
   как подсказка, не как заголовок.
4. **Ответ**: цитаты с диапазонами; для «кто сказал» — диапазон хода
   спикера, для «что сказал» — диапазон speech-сегмента.
5. **Пробелы**: что не анализировалось.

## Pitfalls

- **Два спикера в одной реплике** (`overlap: true`): документ помечает
  перекрытие, но не делит слова. Скажи, что слова не разделены.
- **ASR расслышал иначе**: `pricing` → `preaching` — это реальный класс ошибок.
  Ищи стем, предлагай гипотезу, но не выдавай её за цитату.
- **`speech_duration` ≠ сумма реплик**: первое — измеренная речь по VAD,
  второе — диаризация. Для «кто больше говорил» — только `query speakers`.
- **Границы хода и сегмента расходятся** на ~0.1–0.3 с: не смешивай их в
  одной цитате без пометки.
- **Имена**: в документе их нет. Если пользователь назвал ведущего, всё равно
  говори «S1 (ведущий, по словам пользователя)».

## Long episodes

- Сначала `chapters`, потом цитаты только по нужным главам.
- Не пересказывай всю запись, если спросили про один фрагмент.
- Если файл больше ~2 часов, `standard` может быть медленным: предупреди
  заранее, предложи `structure` как первый проход.
