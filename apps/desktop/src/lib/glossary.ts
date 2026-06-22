export type GlossaryTerm = {
  id: string;
  term: string;
  full?: string;
  short: string;
  definition: string;
  sources?: string[];
};

export const glossaryTerms: GlossaryTerm[] = [
  {
    id: "colecciones",
    term: "Colecciones",
    short:
      "Agrupación lógica de documentos o elementos relacionados dentro de un sistema de almacenamiento o base de datos.",
    definition:
      "Agrupación lógica de documentos o elementos relacionados dentro de un sistema de almacenamiento o base de datos. En el contexto de bases de datos vectoriales y sistemas de recuperación de información, una colección es el contenedor principal que agrupa embeddings, metadatos y documentos indexados que comparten una misma temática o propósito. Permite organizar y consultar grandes volúmenes de información de forma estructurada, separando distintos conjuntos de datos según su uso o dominio.",

    sources: [
      "https://docs.trychroma.com/docs/collections/manage-collections",
      "https://www.ibm.com/think/topics/vector-database",
    ],
  },
  {
    id: "corpus",
    term: "Corpus",
    short:
      "Conjunto grande y estructurado de textos sobre el cual un modelo de lenguaje o sistema de NLP es entrenado, evaluado o consultado.",
    definition:
      "Conjunto grande y estructurado de textos sobre el cual un modelo de lenguaje o sistema de NLP es entrenado, evaluado o consultado. Un corpus puede estar formado por artículos, libros, páginas web, documentos internos u otros textos. Cuanto más representativo y amplio sea el corpus, más preciso tiende a ser el modelo que lo usa. En esta app, el corpus es el conjunto de documentos que el sistema procesa e indexa para poder responder consultas.",

    sources: ["https://en.wikipedia.org/wiki/Text_corpus"],
  },
  {
    id: "embeddings",
    term: "Embeddings",
    short:
      "Representación numérica de un texto como un vector de números, de forma que textos con significado similar quedan cerca entre sí en ese espacio matemático.",
    definition:
      "Representación numérica de un texto como un vector de números, de forma que textos con significado similar quedan cerca entre sí en ese espacio matemático. Los embeddings permiten que las computadoras entiendan el significado del lenguaje sin procesar palabras como texto crudo. Son la base de la búsqueda semántica: en lugar de buscar coincidencias exactas de palabras, el sistema compara el significado de lo que buscás con el significado de los documentos indexados. Son generados por modelos de aprendizaje profundo entrenados sobre grandes volúmenes de texto.",

    sources: ["https://www.ibm.com/think/topics/embedding"],
  },
  {
    id: "entidades",
    term: "Entidades",
    short:
      "Objeto del mundo real identificado y clasificado dentro de un texto, como una persona, organización, lugar, fecha o cantidad.",
    definition:
      "Objeto del mundo real identificado y clasificado dentro de un texto, como una persona, organización, lugar, fecha o cantidad. En procesamiento del lenguaje natural, una entidad nombrada es cualquier elemento del texto que hace referencia a algo concreto y categorizable. Por ejemplo, en la oración Google abrió oficinas en Buenos Aires en 2015, las entidades serían: Google (organización), Buenos Aires (lugar) y 2015 (fecha). Detectar entidades permite estructurar información que originalmente estaba en texto libre. Ver también: NER.",

    sources: ["https://en.wikipedia.org/wiki/Named_entity"],
  },
  {
    id: "faster-whisper",
    term: "Faster-Whisper",
    short:
      "Motor de transcripción de audio a texto, optimizado para ser hasta 4 veces más rápido que el modelo original de OpenAI con la misma precisión y menor uso de memoria.",
    definition:
      "Motor de transcripción de audio a texto, optimizado para ser hasta 4 veces más rápido que el modelo original de OpenAI con la misma precisión y menor uso de memoria. Es una reimplementación del modelo Whisper de OpenAI usando CTranslate2, un motor de inferencia eficiente para modelos Transformer. Funciona tanto en CPU como en GPU, soporta más de 90 idiomas y puede detectar automáticamente el idioma del audio. En esta app se usa para convertir grabaciones o archivos de voz en texto indexable.",

    sources: ["https://github.com/SYSTRAN/faster-whisper"],
  },
  {
    id: "gemma-4",
    term: "Gemma 4",
    short:
      "Familia de modelos de inteligencia artificial de código abierto desarrollados por Google DeepMind, capaces de procesar texto, imágenes y audio.",
    definition:
      "Familia de modelos de inteligencia artificial de código abierto desarrollados por Google DeepMind, capaces de procesar texto, imágenes y audio. Lanzados en abril de 2026 bajo licencia Apache 2.0, los modelos Gemma 4 están disponibles en cinco tamaños y pueden ejecutarse desde teléfonos móviles hasta servidores. Soportan ventanas de contexto de hasta 256K tokens, más de 140 idiomas, y funciones avanzadas como razonamiento paso a paso, llamadas a herramientas y comprensión de documentos. Al ser de código abierto, pueden usarse de forma local sin depender de servicios externos.",

    sources: ["https://ai.google.dev/gemma/docs/core"],
  },
  {
    id: "indexacion-fts",
    term: "Indexación FTS",
    full: "Full-Text Search",
    short:
      "Técnica de búsqueda que permite encontrar documentos por su contenido textual de forma rápida, sin necesidad de recorrer todos los registros uno por uno.",
    definition:
      "FTS (Full-Text Search) es una extensión de SQLite que construye un índice invertido: un mapa de cada palabra hacia los documentos que la contienen. Cuando el usuario realiza una búsqueda, el sistema consulta ese índice en lugar de escanear todos los textos, lo que lo hace órdenes de magnitud más rápido. Soporta búsquedas por relevancia, frases exactas y coincidencias parciales.",

    sources: ["https://en.wikipedia.org/wiki/Full-text_search"],
  },
  {
    id: "llm",
    term: "LLM",
    full: "Large Language Model",
    short:
      "Modelo de inteligencia artificial entrenado sobre enormes volúmenes de texto para comprender y generar lenguaje humano con alta coherencia y precisión.",
    definition:
      "Los LLMs están basados en arquitectura Transformer y aprenden patrones del lenguaje a partir de miles de millones de ejemplos. Son capaces de responder preguntas, resumir documentos, traducir idiomas, escribir código y mucho más. Su comportamiento depende del entrenamiento recibido y del texto (prompt) que se les proporciona como entrada. Ejemplos conocidos: GPT-4, Claude, Gemini, Llama.",

    sources: ["https://www.ibm.com/think/topics/large-language-models"],
  },
  {
    id: "llm-local",
    term: "LLM Local",
    short:
      "LLM que se ejecuta íntegramente en el propio dispositivo del usuario, sin enviar datos a servicios externos.",
    definition:
      "A diferencia de los LLMs en la nube, un LLM local procesa toda la información internamente, lo que garantiza privacidad total de los datos y funcionamiento sin conexión a internet. La contrapartida es que requiere hardware suficiente (RAM y/o GPU) para correr el modelo. En esta app, elegir un LLM local significa que ningún documento ni consulta sale del entorno controlado del usuario.",

    sources: ["https://ollama.com", "https://lmstudio.ai"],
  },
  {
    id: "llm-cloud",
    term: "LLM Cloud",
    short:
      "LLM alojado en servidores de un proveedor externo, al que la app accede mediante una conexión de red.",
    definition:
      "Los LLMs en la nube como GPT-4 (OpenAI), Claude (Anthropic) o Gemini (Google) ofrecen modelos de última generación sin necesidad de infraestructura propia. Son más potentes y fáciles de escalar, pero los datos enviados en cada consulta son procesados por terceros. En esta app, elegir un LLM Cloud implica que los textos consultados viajan hacia los servidores del proveedor seleccionado.",

    sources: [
      "https://platform.openai.com/docs/overview",
      "https://docs.anthropic.com",
    ],
  },
  {
    id: "nlp",
    term: "NLP",
    full: "Natural Language Processing",
    short:
      "Campo de la inteligencia artificial que desarrolla técnicas para que las computadoras puedan leer, interpretar y generar lenguaje humano.",
    definition:
      "NLP combina lingüística computacional con aprendizaje automático para analizar texto y voz. Sus aplicaciones incluyen búsqueda semántica, análisis de sentimiento, traducción automática, resumen de documentos, extracción de información y chatbots. La mayoría de las funciones de esta app que trabajan con texto —como la indexación, búsqueda y extracción de entidades— están construidas sobre técnicas de NLP.",

    sources: ["https://www.ibm.com/think/topics/natural-language-processing"],
  },
  {
    id: "ocr",
    term: "OCR",
    full: "Optical Character Recognition",
    short:
      "Tecnología que convierte imágenes que contienen texto en texto digital editable y buscable.",
    definition:
      "OCR analiza los píxeles de una imagen para identificar caracteres, palabras y oraciones, y los transforma en texto que la computadora puede procesar. Los sistemas modernos usan redes neuronales profundas para reconocer fuentes variadas, escritura a mano y documentos de baja calidad. En esta app, OCR es el primer paso para procesar cualquier documento que no venga en formato de texto nativo.",

    sources: [
      "https://www.ibm.com/think/topics/optical-character-recognition",
    ],
  },
  {
    id: "ocr-high",
    term: "OCR High",
    short:
      "Modalidad de OCR de máxima precisión, diseñada para documentos difíciles donde se prioriza la fidelidad del texto sobre la velocidad.",
    definition:
      "OCR High aplica modelos más complejos y mayor resolución de análisis para obtener resultados exactos en documentos con tipografías inusuales, baja calidad de imagen, escritura a mano, tablas complejas o contenido técnico. Es la opción recomendada cuando los errores de extracción tienen un alto costo (documentos legales, médicos, técnicos). Generalmente más lento que OCR Light.",

    sources: [
      "https://github.com/PaddlePaddle/PaddleOCR",
      "https://tesseract-ocr.github.io/tessdoc/",
    ],
  },
  {
    id: "ocr-light",
    term: "OCR Light",
    short:
      "Modalidad de OCR optimizada para velocidad, adecuada para documentos estándar con buena calidad de imagen y tipografía clara.",
    definition:
      "OCR Light usa modelos más ligeros que consumen menos recursos computacionales, lo que permite procesar grandes volúmenes de documentos de forma rápida. Es la opción recomendada para textos impresos en fuentes comunes, con buena iluminación y sin elementos complejos. Puede presentar menor precisión en documentos deteriorados o con diseños inusuales.",

    sources: [
      "https://github.com/PaddlePaddle/PaddleOCR",
      "https://tesseract-ocr.github.io/tessdoc/",
    ],
  },
  {
    id: "paddleocr-vl",
    term: "PaddleOCR-VL",
    short:
      "Modelo de visión y lenguaje especializado en el análisis de documentos complejos, capaz de extraer texto, tablas, fórmulas y gráficos con alta precisión en más de 100 idiomas.",
    definition:
      "Desarrollado por PaddlePaddle (Baidu), PaddleOCR-VL combina un encoder visual de resolución dinámica con un modelo de lenguaje ligero para entender el contenido y la estructura de documentos. Con solo 0.9B parámetros, supera en benchmarks a modelos mucho más grandes. Está diseñado para escenarios reales como documentos escaneados, fotografiados en pantalla, con iluminación irregular o páginas inclinadas.",

    sources: ["https://github.com/PaddlePaddle/PaddleOCR"],
  },
  {
    id: "spacy",
    term: "spaCy",
    short:
      "Librería de Python de código abierto para procesamiento del lenguaje natural, diseñada para aplicaciones reales que requieren velocidad y precisión en producción.",
    definition:
      "spaCy ofrece componentes listos para usar como reconocimiento de entidades (NER), etiquetado gramatical, análisis sintáctico, lematización y vectores de palabras. A diferencia de otras librerías orientadas a investigación, su diseño prioriza la eficiencia y la facilidad de integración en sistemas de software. Es una de las herramientas de NLP más utilizadas en la industria.",

    sources: ["https://spacy.io/usage/spacy-101"],
  },
  {
    id: "sqlite",
    term: "SQLite",
    short:
      "Motor de base de datos relacional autocontenido que funciona como una biblioteca dentro de la misma aplicación, sin necesidad de un servidor separado.",
    definition:
      "SQLite almacena toda la base de datos en un único archivo de disco, lo que lo hace ideal para aplicaciones que necesitan persistencia de datos sin la complejidad de configurar un servidor. Es el motor de base de datos más desplegado del mundo: está integrado en todos los teléfonos móviles, la mayoría de los navegadores web y miles de aplicaciones de escritorio. En esta app se usa para almacenar y consultar documentos, metadatos e índices de búsqueda.",

    sources: ["https://sqlite.org/about.html"],
  },
  {
    id: "tesseract",
    term: "Tesseract",
    short:
      "Motor de OCR de código abierto, uno de los más precisos y utilizados del mundo, que convierte imágenes con texto en texto digital.",
    definition:
      "Desarrollado originalmente por Hewlett-Packard entre 1985 y 1994, y mantenido por Google desde 2006 hasta 2017, Tesseract es hoy mantenido por la comunidad open source. Su versión actual (5.x) incorpora reconocimiento basado en redes neuronales LSTM, lo que le permite manejar más de 100 idiomas con alta precisión. Es una de las bases sobre la que se construyen los flujos de OCR de esta app.",

    sources: ["https://tesseract-ocr.github.io/tessdoc/Home.html"],
  },
  {
    id: "triples",
    term: "Triples",
    short:
      "Unidad básica para representar conocimiento en forma de una afirmación de tres partes: quién, qué relación tiene, y con qué.",
    definition:
      "Un triple está formado por Sujeto, Predicado y Objeto: por ejemplo, (Madrid, es_capital_de, España). Esta estructura, definida por el estándar RDF del W3C, permite representar cualquier hecho o relación de forma que tanto humanos como máquinas puedan interpretarlo. Un conjunto de triples forma un grafo de conocimiento, donde los sujetos y objetos son nodos y los predicados son las conexiones entre ellos.",

    sources: ["https://www.w3.org/TR/rdf12-concepts/"],
  },
  {
    id: "triples-spo",
    term: "Triples S-P-O",
    short:
      "Notación que describe la estructura de un triple: Sujeto, Predicado y Objeto.",
    definition:
      "Esta nomenclatura viene del estándar RDF (Resource Description Framework) del W3C y es la forma canónica de expresar hechos en grafos de conocimiento. Por ejemplo: S=Ada Lovelace → P=es_conocida_como → O=primera programadora. Cada parte del triple puede ser una entidad, un concepto o un valor literal. Al encadenar múltiples triples se construye una red semántica que permite razonar sobre relaciones complejas.",

    sources: ["https://www.w3.org/TR/rdf12-concepts/"],
  },
];

export function getTermById(id: string): GlossaryTerm | undefined {
  return glossaryTerms.find((t) => t.id === id);
}
