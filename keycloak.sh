docker run -p 8080:8080 -d --name keycloak \
    -v $(pwd)/test-data/keycloak:/opt/keycloak/data \
    -e KC_BOOTSTRAP_ADMIN_USERNAME=admin -e KC_BOOTSTRAP_ADMIN_PASSWORD=admin \
    quay.io/keycloak/keycloak:26.1.2 \
    start-dev
