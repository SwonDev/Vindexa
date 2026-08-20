//! Qué modelo proponerle a esta máquina.
//!
//! # Por qué no hay una lista escrita a mano
//!
//! Una lista de modelos recomendados envejece en semanas: salen versiones
//! nuevas, las cuantizaciones cambian de nombre y los repositorios se mueven. Un
//! catálogo fijo dentro de la aplicación acabaría recomendando cosas que ya no
//! existen, y recomendar algo que no existe es peor que no recomendar nada.
//!
//! Aquí se pregunta a Hugging Face en el momento, filtrando por lo que la
//! máquina puede mover. Los nombres salen de allí, nunca de aquí.
//!
//! # Cómo se decide el tamaño
//!
//! De la memoria que se puede dedicar a un modelo, que a su vez sale de
//! preguntarle al sistema cuánta hay. Sin ese dato no se propone nada: sugerir
//! un modelo de treinta mil millones de parámetros a un portátil de ocho gigas
//! es hacerle perder una tarde de descarga para nada.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Cuánto se espera a Hugging Face. Es una sugerencia, no una función crítica:
/// más de esto y se prefiere decir que no se pudo consultar.
const TIMEOUT: Duration = Duration::from_secs(12);

/// Cuántos se ofrecen. Los suficientes para elegir, no tantos como para tener
/// que estudiárselos.
const HOW_MANY: usize = 6;

/// Clase de tamaño que le va a una máquina, con el término por el que se busca.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeClass {
    /// Memoria mínima que pide, en bytes.
    pub needs_bytes: u64,
    /// Cómo se llama en los nombres de los repositorios.
    pub token: &'static str,
    /// Cómo se le explica a una persona.
    pub label: &'static str,
}

/// De mayor a menor. Se elige la primera que quepa.
const CLASSES: &[SizeClass] = &[
    SizeClass {
        needs_bytes: 24 * 1024 * 1024 * 1024,
        token: "30B",
        label: "unos 30 000 millones de parámetros",
    },
    SizeClass {
        needs_bytes: 12 * 1024 * 1024 * 1024,
        token: "14B",
        label: "unos 14 000 millones de parámetros",
    },
    SizeClass {
        needs_bytes: 6 * 1024 * 1024 * 1024,
        token: "8B",
        label: "unos 8 000 millones de parámetros",
    },
    SizeClass {
        needs_bytes: 3 * 1024 * 1024 * 1024,
        token: "4B",
        label: "unos 4 000 millones de parámetros",
    },
    SizeClass {
        needs_bytes: 0,
        token: "1.7B",
        label: "pequeño, para máquinas justas de memoria",
    },
];

/// Qué clase de modelo le va a una máquina con esta memoria disponible.
///
/// `None` cuando no se sabe cuánta memoria hay: entonces no se propone nada.
pub fn size_class(usable_bytes: Option<u64>) -> Option<SizeClass> {
    let usable = usable_bytes?;
    CLASSES
        .iter()
        .find(|class| usable >= class.needs_bytes)
        .copied()
}

/// Un modelo que se puede descargar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelSuggestion {
    /// Identificador del repositorio: `autor/nombre`.
    pub repo: String,
    /// Cuánta gente lo usa. Es lo más parecido a «esto funciona» que hay.
    pub downloads: u64,
    pub likes: u64,
}

/// Lo que se propone, con el porqué.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSuggestions {
    /// Explicación del tamaño elegido. Vacía si no se pudo decidir.
    pub rationale: String,
    pub suggestions: Vec<ModelSuggestion>,
}

#[derive(Deserialize)]
struct HubModel {
    #[serde(default)]
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
}

/// Pregunta a Hugging Face qué hay para esta máquina.
///
/// Devuelve sólo repositorios con formato GGUF, que es lo que llama.cpp sabe
/// cargar y lo que hace que la propuesta sea utilizable de verdad.
pub async fn suggest(usable_bytes: Option<u64>) -> AppResult<CatalogSuggestions> {
    let Some(class) = size_class(usable_bytes) else {
        return Ok(CatalogSuggestions {
            rationale: String::new(),
            suggestions: Vec::new(),
        });
    };
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|error| AppError::new("model_catalog", format!("Cliente HTTP: {error}")))?;
    let response = client
        .get("https://huggingface.co/api/models")
        .query(&[
            ("filter", "gguf"),
            ("search", &format!("{} instruct", class.token)),
            ("sort", "downloads"),
            ("direction", "-1"),
            ("limit", &(HOW_MANY * 2).to_string()),
        ])
        .send()
        .await
        .map_err(|error| {
            AppError::new(
                "model_catalog",
                format!("No se pudo consultar Hugging Face: {error}"),
            )
        })?
        .error_for_status()
        .map_err(|error| {
            AppError::new(
                "model_catalog",
                format!("Hugging Face respondió con error: {error}"),
            )
        })?;
    let hub: Vec<HubModel> = response.json().await.map_err(|error| {
        AppError::new(
            "model_catalog",
            format!("No se entendió la respuesta de Hugging Face: {error}"),
        )
    })?;

    let suggestions = hub
        .into_iter()
        .filter(|model| !model.id.is_empty())
        // La búsqueda es aproximada: se comprueba que el nombre lleve de verdad
        // el tamaño buscado, para no proponer un 235B a quien pidió un 8B.
        .filter(|model| {
            model
                .id
                .to_uppercase()
                .contains(&class.token.to_uppercase())
        })
        .take(HOW_MANY)
        .map(|model| ModelSuggestion {
            repo: model.id,
            downloads: model.downloads,
            likes: model.likes,
        })
        .collect();

    Ok(CatalogSuggestions {
        rationale: format!(
            "Con la memoria de esta máquina entra un modelo de {}. Todos son GGUF, que es lo que carga llama.cpp.",
            class.label
        ),
        suggestions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn sin_saber_la_memoria_no_se_propone_nada() {
        // Recomendar a ciegas es hacer perder una tarde de descarga.
        assert!(size_class(None).is_none());
    }

    #[test]
    fn cada_maquina_recibe_lo_que_puede_mover() {
        assert_eq!(size_class(Some(48 * GIB)).map(|c| c.token), Some("30B"));
        assert_eq!(size_class(Some(24 * GIB)).map(|c| c.token), Some("30B"));
        assert_eq!(size_class(Some(16 * GIB)).map(|c| c.token), Some("14B"));
        assert_eq!(size_class(Some(8 * GIB)).map(|c| c.token), Some("8B"));
        assert_eq!(size_class(Some(4 * GIB)).map(|c| c.token), Some("4B"));
        assert_eq!(size_class(Some(GIB)).map(|c| c.token), Some("1.7B"));
    }

    #[test]
    fn las_clases_van_de_mayor_a_menor() {
        // El orden es lo que hace correcto el «primero que quepa»: si se
        // desordenan, una máquina grande recibiría el modelo más pequeño.
        for pair in CLASSES.windows(2) {
            assert!(
                pair[0].needs_bytes > pair[1].needs_bytes,
                "{} no pide más que {}",
                pair[0].token,
                pair[1].token
            );
        }
        assert_eq!(
            CLASSES.last().map(|class| class.needs_bytes),
            Some(0),
            "siempre tiene que haber una opción para la máquina más justa"
        );
    }
}
