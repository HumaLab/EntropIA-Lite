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

## Sincronización en la nube (opcional)

La sincronización multi-dispositivo es **opt-in**: no viaja nada al servidor hasta que iniciás sesión en una cuenta de sync. Si nunca la activás, esta sección no aplica y el resto de la app funciona igual, 100% local.

Cuando la activás, tené en cuenta:

| Tema | Detalle |
| ---- | ------- |
| Qué viaja | Las filas de las 15 tablas sincronizadas (colecciones, ítems, assets, notas, anotaciones, extracciones OCR, transcripciones, layouts, entidades, triples, topics, asociaciones, resultados LLM y conversaciones/mensajes RAG) y los archivos asociados (imágenes, PDFs renderizados, audio). **No viajan** `app_settings`, embeddings vectoriales, FTS ni el historial de undo de imágenes (`_vN`). |
| Sin cifrado de extremo a extremo | El servidor **ve** tus datos. La protección en tránsito es TLS (HTTPS obligatorio salvo `localhost`); no hay E2E en v1. Sincronizá solo contra un servidor que controlés o en el que confíes. |
| Journal de conflictos | Ante una edición concurrente, la resolución es "última escritura gana" por fila. La versión **perdedora se guarda completa** en un journal local de conflictos para que no se pierda nada y puedas revisarla. Ese payload perdedor queda en tu base local hasta que lo borres o cierres la sesión. |
| Persistencia de blobs | Los archivos subidos **persisten en el servidor hasta que borres la cuenta**. No hay recolección automática: borrar un asset localmente no borra su blob del servidor. |
| Borrado de cuenta | "Borrar mis datos del servidor" (con confirmación por password) elimina filas, conflictos, metadata de blobs, contadores y dispositivos, y borra el directorio de blobs de tu cuenta. Tus datos **locales** quedan intactos. |
| Logs del servidor | El servidor registra por request la cuenta y el dispositivo (id, no el token) para diagnóstico y observabilidad. |
| Backups del operador | Si el operador del servidor corre backups (Litestream para la base, restic/rclone para los blobs), esos backups **retienen datos borrados hasta su rotación**. El borrado de cuenta no purga réplicas ni snapshots de backup. |
| Token de dispositivo | Cada login crea un dispositivo nuevo con un token opaco guardado **solo en el keyring del SO**. El token nunca se loguea ni se guarda en la base. Podés revocar dispositivos desde la app. |

## Términos de proveedores

Los proveedores remotos tienen sus propias políticas de privacidad, reglas de retención y controles de cuenta. Revisá los términos de OpenRouter, Z.ai/GLM-OCR y AssemblyAI antes de procesar material sensible.
