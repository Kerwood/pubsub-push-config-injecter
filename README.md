# PubSub Push Config Injecter
[![forthebadge made-with-rust](http://ForTheBadge.com/images/badges/made-with-rust.svg)](https://www.rust-lang.org/)

![Image Size](https://ghcr-badge.egpl.dev/kerwood/pubsub-push-config-injecter/size?tag=latest)

When deploying a Google Cloud PubSubSubscription with a pushConfig using Config Connector in GKE, the `pushConfig.endpoint` value can contain credentials.
This little application is a Kubernetes Admission Controller that mutates the PubSubSubscription object and injects the `pushConfig.endpoint` value from a secret.

All you have to do is:
- Create a secret with the endpoint URL.
- Install the controller using Helm.
- Set a lable on one or more namepaces that the controller should intercept.
- Deploy a PubSubSubscription with the needed annotaion.

## Prerequisites

For deploying:
 - Helm, <https://helm.sh/docs/intro/install/>
 - Kubectl, <https://kubernetes.io/docs/tasks/tools/>

For developing
 - Kind, <https://kind.sigs.k8s.io/docs/user/quick-start/>
 - Teleprecense, <https://www.getambassador.io/docs/telepresence/latest/install>
 - Rust, <https://www.rust-lang.org/tools/install>
 - Just, <https://github.com/casey/just>
 - OpenSSL (*If you want to create a new CA*)


## Install with default CA certificates

Add the Helm repository and update.
```sh
helm repo add kerwood https://kerwood.github.io/helm-charts
helm repo update
```

Install the Push Config Injecter.
```sh
helm install push-config-connector kerwood/pubsub-push-config-injecter \
  --namespace <your-namespace>
```

## Install with your own CA certificates

The application will create a new certificate and key from the CA every time it starts.
The CA certificate and key therfore needs be mounted inside the pod. 

Either bring your own cert and key or generate a new set with below command.
```sh
just gen-ca
```

Create a Kubernetes secret with the new CA cert and key. 
```sh
kubectl create secret tls push-config-injecter-certs \
  --cert=./certs/ca.crt \
  --key=./certs/ca.key
```

Add the Helm repository and update.
```sh
helm repo add kerwood https://kerwood.github.io/helm-charts
helm repo update
```

Install with Helm and set the `tlsSecretName` and `webhook.ca`.
```sh
helm install push-config-injecter kerwood/pubsub-push-config-injecter \
  --set controller.tlsSecretName=push-config-injecter-certs \
  --set webhook.ca="$(cat certs/ca.crt)" \
  --namespace <your-namespace>
```

## How to use it?

Add a label to a namespace and the controller will start intercepting PubSubSubscription objects.
```sh
kubectl label namespace <your-namepace> pubsub-push-config-injecter=enabled
```

Create a secret with the endpoint value in your desired namespace.
```sh
kubectl create secret generic datadog-push-config \
  --from-literal="endpoint=https://gcp-intake.logs.datadoghq.eu/api/v2/logs?dd-api-key=xxxxxxxxxxx" \
  --namespace default
```

Add an annotation to your PubSubSubscription that points to the secret key that holds the endpoint value.
The annotation value shold be `<namespace>/<secret-name>/<secret-key>`.
```yaml
apiVersion: pubsub.cnrm.cloud.google.com/v1beta1
kind: PubSubSubscription
metadata:
  name: some-subscription-name
  annotations:
    pubsub-push-config/inject-from: default/datadog-push-config/endpoint
spec:
  topicRef:
    name: some-topic-name
```

When the object is created in Kubernetes it should end up looking like this.
```yaml
apiVersion: pubsub.cnrm.cloud.google.com/v1beta1
kind: PubSubSubscription
metadata:
  name: some-subscription-name
  namespace: default
  annotations:
    pubsub-push-config/inject-from: default/datadog-push-config/endpoint
spec:
  pushConfig:
    pushEndpoint: https://gcp-intake.logs.datadoghq.eu/api/v2/logs?dd-api-key=xxxxxxxxxxx
  topicRef:
    name: some-topic-name
```

## Development environment
Want to test it out locally, no problem.
The Just file have different recipes to help with that.
```
Available recipes:
    all                       # Cluster up, generate certs, build dev image, deploy to kind and intercept traffic
    build-image tag="latest"  # build container image for release
    build-image-dev tag="dev" # Build a dev image on debian:sid-slim
    cluster-down              # Bring down the Kind cluster
    cluster-up                # Bring up the Kind cluster
    deploy-dev tag="dev"      # Deploy Webhook, Certificates and Deployment to the Kind cluster
    gen-ca                    # Genereate CA and certificate for the controller
    load-image tag="dev"      # Load the container image into Kind
    push-image tag="latest"   # build container image for release
    tp-intercept              # Intercept webhook traffic from the Kind cluster
```

Make sure you have the prerequisites installed for developing and run `just all`.

This will run the following:
- cluster-up:
  - Creates a Kind cluster.
  - Installs the PubSubSubscription CRD.
  - Installs Teleprecense in the cluster.
- tp-intercept:
  - Builds a dev image.
  - Loads the image into Kind.
  - Installs the pubsub-push-config-injecter.
  - Starts intercepting traffic from the pod with Teleprecense.
  - Runs `cargo run`

  Now try and deploy a PubSubSubscription.
  ```
  kubectl apply -f manifests/pubsub-subscription-example.yaml
  ```
