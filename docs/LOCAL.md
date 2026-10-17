# Running OwnVoice Meet against your own servers

OwnVoice Meet talks to local servers through the same doors as the cloud: an OpenAI-compatible transcription endpoint for listening, and an OpenAI-compatible chat endpoint for translation. There is no separate local code path; a local setup is a URL and a model name in **Settings**.

Audio and text then stay on your machine. OwnVoice Meet does not use a system proxy, so `localhost` addresses are reached directly.

## Listening: a local Whisper server

OwnVoice Meet posts each sentence as a 16 kHz mono WAV file to `POST <server>/audio/transcriptions` (multipart fields `file`, `model`, `language`, `response_format=json`) and reads `text` from the JSON reply. Any server that does that works.

### The bundled minimal server (tested)

`scripts/local_whisper_server.py` is about a hundred lines on top of [faster-whisper](https://github.com/SYSTRAN/faster-whisper), standard library otherwise.

```
pip install faster-whisper
python scripts/local_whisper_server.py --model large-v3 --device cuda --port 8790
```

- `--device cpu --compute-type int8` runs without an NVIDIA GPU (slower).
- The first request after start loads the model into the GPU and takes a few seconds; later ones are fast. Measured on an RTX 3090 with `large-v3`: a 5-second sentence took 2.9 s on the first request and 0.68 s after.
- Pick a free port. If a request comes back with a reply that is not `{"text": ...}`, another program is listening on that port.

Then in Settings:

| Field | Value |
|---|---|
| 聽：語音辨識 | 本機 Whisper（OpenAI 相容伺服器） |
| 伺服器網址 | `http://127.0.0.1:8790/v1` |
| 模型名稱 | anything (the bundled server always uses the model it was started with) |

### Other servers (not tested by us)

Servers that implement the OpenAI transcription API should work the same way: put their base URL (ending in `/v1`) and their model name in Settings. If one needs a key, enter it under **本機伺服器（選填）**.

## Translation: a local language model

OwnVoice Meet sends Chat Completions requests with streaming to `POST <server>/chat/completions`. vLLM, Ollama and LM Studio all serve this API. Not yet tested by us; please report what works.

| Server | Start | Settings 伺服器網址 | 模型名稱 |
|---|---|---|---|
| vLLM | `vllm serve <model>` | `http://localhost:8000/v1` | the model you served |
| Ollama | `ollama pull <model>` (Ollama serves on start) | `http://localhost:11434/v1` | e.g. the tag you pulled |

Small local models vary a lot in Traditional Chinese and Taiwanese usage. Try a few sentences from a real meeting before relying on one; the glossary in Settings helps with names and terms.

## Apple devices (planned)

On-device speech recognition (SpeechAnalyzer) and translation (Foundation Models) need macOS 26 on Apple Silicon. They appear in the switcher as planned and are not built yet; macOS system-audio capture comes first.

## Checking a setup

`scripts/e2e-listen.ps1` launches OwnVoice Meet with a settings folder of your choice, presses Listen, plays two English sentences through the speakers with Windows text-to-speech, and prints the captions:

```
powershell -ExecutionPolicy Bypass -File scripts\e2e-listen.ps1 -Exe src-tauri\target\debug\ownvoice-meet.exe -ConfigDir <folder with settings.json>
```
