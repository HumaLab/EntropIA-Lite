# Aviso de privacidad de EntropIA Lite

Español | [English](PRIVACY.en.md)

EntropIA Lite es una app desktop con datos locales y proveedores remotos de IA. Tus datos de trabajo viven en tu máquina, pero las funciones de OCR, transcripción, LLM, embeddings y NER envían el contenido seleccionado a APIs externas cuando las ejecutás.

## Datos locales

| Dato | Tratamiento |
| ---- | ----------- |
| Colecciones, ítems, metadata, notas y anotaciones | Se guardan en la base local de la app. |
| Assets importados | Se referencian o copian según el flujo de importación desktop. |
| Texto OCR, transcripciones, entidades, triples, resúmenes, FTS y vectores | Se guardan localmente después de que termina el procesamiento del proveedor. |
| Logs operativos | Se escriben localmente para diagnóstico. Revisalos antes de compartirlos. |

## Procesamiento remoto

| Función | Proveedor | Qué puede enviarse |
| ------- | --------- | ------------------ |
| Tareas LLM, resúmenes, correcciones, triples | OpenRouter | Texto de prompt y contexto documental relevante. |
| Embeddings | OpenRouter | Texto para vectorizar. |
| NER | OpenRouter | Texto para analizar entidades. |
| OCR | GLM-OCR / Z.ai | Imagen o PDF seleccionado. |
| Transcripción | AssemblyAI | Audio seleccionado. |

El perfil Lite actual no publica ni requiere descargas de modelos locales para GGUF/llama, Paddle/PaddleVL, ONNX/tokenizers, faster-whisper o spaCy.

## API keys

Las claves de OpenRouter, GLM-OCR/Z.ai y AssemblyAI son secretos provistos por el usuario. La app guarda referencias a secretos en la configuración local y resuelve los valores reales mediante el keyring nativo del sistema cuando está disponible.

Tratá las claves como credenciales:

- no incluyas datos de la app, bases locales, logs ni snapshots de configuración en commits;
- revisá los diagnósticos antes de compartirlos;
- rotá cualquier clave que pueda haberse expuesto.

## Control del usuario

- No configures la clave de un proveedor si no querés usar ese proveedor.
- Quitá la clave desde Configuración para deshabilitar esa ruta remota.
- Eliminá el directorio de datos local de la app si querés borrar bases, logs, salidas generadas y referencias de configuración.

## Términos de proveedores

Los proveedores remotos tienen sus propias políticas de privacidad, reglas de retención y controles de cuenta. Revisá los términos de OpenRouter, Z.ai/GLM-OCR y AssemblyAI antes de procesar material sensible.
