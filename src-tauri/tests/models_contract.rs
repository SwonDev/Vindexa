#[allow(dead_code)]
#[path = "../src/models.rs"]
mod models;

use models::{
    BulkUpdateStatusInput, GameListRequest, MovePlannerItemInput, RecommendationRequest,
    UpdateGameInput,
};
use serde_json::json;

#[test]
fn game_list_request_uses_frontend_camel_case_contract() {
    let request = GameListRequest {
        query: Some("aventura".to_string()),
        status_id: Some("playing".to_string()),
        collection_id: Some("favorites".to_string()),
        installed: Some(true),
        tracking: Some(false),
        early_access: Some(true),
        is_free: Some(true),
        ownership_source: Some("owned".to_string()),
        sort: Some("lastPlayed".to_string()),
        sort_seed: None,
        limit: Some(60),
        offset: Some(120),
        ..GameListRequest::default()
    };

    assert_eq!(
        serde_json::to_value(request).expect("serializar filtro"),
        json!({
            "query": "aventura",
            "statusId": "playing",
            "collectionId": "favorites",
            "installed": true,
            "tracking": false,
            "earlyAccess": true,
            "isFree": true,
            "ownershipSource": "owned",
            "sort": "lastPlayed",
            "limit": 60,
            "offset": 120
        })
    );
}

#[test]
fn random_sort_seed_uses_the_exact_frontend_field_name() {
    let request: GameListRequest = serde_json::from_value(json!({
        "sort": "random",
        "sortSeed": 42,
        "limit": 120,
        "offset": 0
    }))
    .expect("deserializar orden aleatorio");

    assert_eq!(request.sort.as_deref(), Some("random"));
    assert_eq!(request.sort_seed, Some(42));
}

#[test]
fn advanced_filters_use_the_exact_frontend_field_names() {
    let payload = json!({
        "neverPlayed": false,
        "minPlaytimeMinutes": 60,
        "maxPlaytimeMinutes": 600,
        "minProgress": 10,
        "maxProgress": 90,
        "minRating": 4,
        "maxRating": 10,
        "genre": "Acción",
        "category": "Cooperativo",
        "tagId": "relajante",
        "releaseFrom": "2020-01-01",
        "releaseTo": "2026-12-31",
        "lastPlayedFrom": "2026-01-01",
        "lastPlayedTo": "2026-08-14",
        "minAchievementPercent": 20,
        "maxAchievementPercent": 80,
        "steamDeckStatus": "verified",
        "targetDateFrom": "2026-08-01",
        "targetDateTo": "2026-12-31",
        "minSessionMinutes": 30,
        "maxSessionMinutes": 120
    });
    let request: GameListRequest =
        serde_json::from_value(payload.clone()).expect("deserializar filtros avanzados");

    assert!(!request.never_played.expect("triestado false"));
    assert_eq!(request.min_playtime_minutes, Some(60));
    assert_eq!(request.steam_deck_status.as_deref(), Some("verified"));
    let serialized = serde_json::to_value(request).expect("serializar filtros avanzados");
    for key in payload.as_object().expect("objeto de filtros").keys() {
        assert_eq!(&serialized[key], &payload[key], "campo {key}");
    }
}

#[test]
fn update_game_input_deserializes_the_exact_tauri_payload() {
    let payload = json!({
        "appId": 620,
        "statusId": "playing",
        "progress": 55,
        "priority": 4,
        "pinned": true,
        "tracking": true,
        "rating": 9,
        "estimatedMinutes": 180,
        "targetDate": "2026-09-01",
        "nextAction": "Terminar el segundo acto",
        "checkpoint": "Campamento",
        "notes": "Decisiones pendientes"
    });

    let input: UpdateGameInput = serde_json::from_value(payload).expect("deserializar edición");

    assert_eq!(input.app_id, 620);
    assert_eq!(input.status_id, "playing");
    assert_eq!(input.progress, 55);
    assert_eq!(input.priority, 4);
    assert!(input.pinned);
    assert!(input.tracking);
    assert_eq!(input.rating, Some(9));
    assert_eq!(input.estimated_minutes, Some(180));
    assert_eq!(input.target_date.as_deref(), Some("2026-09-01"));
}

#[test]
fn bulk_status_input_deserializes_the_exact_tauri_payload() {
    let input: BulkUpdateStatusInput = serde_json::from_value(json!({
        "appIds": [10, 20, 30],
        "statusId": "playing"
    }))
    .expect("deserializar cambio masivo");

    assert_eq!(input.app_ids, vec![10, 20, 30]);
    assert_eq!(input.status_id, "playing");
}

#[test]
fn planner_and_recommendation_payloads_reject_wrong_field_names() {
    let snake_case_move = json!({
        "app_id": 10,
        "column_id": "now",
        "position": 0
    });
    let valid_move = json!({
        "appId": 10,
        "columnId": "now",
        "position": 0
    });

    assert!(serde_json::from_value::<MovePlannerItemInput>(snake_case_move).is_err());
    let parsed = serde_json::from_value::<MovePlannerItemInput>(valid_move)
        .expect("deserializar movimiento válido");
    assert_eq!(parsed.app_id, 10);
    assert_eq!(parsed.column_id, "now");

    let recommendation: RecommendationRequest = serde_json::from_value(json!({
        "durationMinutes": 45,
        "mood": "relajado"
    }))
    .expect("deserializar recomendación");
    assert_eq!(recommendation.duration_minutes, Some(45));
    assert_eq!(recommendation.mood.as_deref(), Some("relajado"));
}
