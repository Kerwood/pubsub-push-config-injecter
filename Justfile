cluster_name := "admission-controller"
image_name := "ghcr.io/kerwood/push-config-injecter"

[private]
default:
	@just -l

# Bring up the Kind cluster
cluster-up:
	kind create cluster --name {{cluster_name}} --image kindest/node:v1.26.2  --config ./kind-config.yaml
	sleep "10"
	kubectl wait --namespace kube-system --for=condition=ready pod --selector="tier=control-plane" --timeout=180s
	kubectl apply -f ./manifests/crd-pubsubsubscriptions.pubsub.cnrm.cloud.google.com.yaml
	kubectl label namespace default pubsub-push-config-injecter=enabled --overwrite=true
	-telepresence helm install
	-telepresence connect

# Bring down the Kind cluster
cluster-down:
	-telepresence quit
	kind delete cluster --name {{cluster_name}}
	-rm ./kubeconfig

# Intercept webhook traffic from the Kind cluster
tp-intercept: build-image-dev (deploy-dev "dev") (load-image "dev")
	telepresence intercept push-config-injecter-controller --port 8443:443
	RUST_LOG="push_config_injecter=debug" cargo run

# Build a dev image on debian:sid-slim
build-image-dev tag="dev":
	cargo build 
	docker build -f Dockerfile.dev -t {{image_name}}:{{tag}} .
	
# build container image for release
build-image tag="latest":
	docker build -t {{image_name}}:{{tag}} .

# build container image for release
push-image tag="latest":
	docker push {{image_name}}:{{tag}}

# Load the container image into Kind
load-image tag="dev":
	kind --name {{cluster_name}} load docker-image {{image_name}}:{{tag}}

# Genereate CA and certificate for the controller
gen-ca:
	openssl genrsa -out certs/ca.key 2048
	openssl req -new -x509 -key certs/ca.key -out certs/ca.crt -days 3650 -config certs/ca.conf
	cp -v certs/ca.crt chart/ca/

# Deploy Webhook, Certificates and Deployment to the Kind cluster
@deploy-dev tag="dev":
	kubectl apply -f ./manifests/secret-push-endpoint.yaml
	helm upgrade --install pubsub-push-config-injecter ./chart \
		--set webhook.ca="$(cat certs/ca.crt)" \
		--set controller.image={{image_name}} \
		--set controller.tag={{tag}}

# Cluster up, generate certs, build dev image, deploy to kind and intercept traffic
all: cluster-up tp-intercept

