# Build stage
FROM node:20-alpine AS builder
WORKDIR /build
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
ARG VITE_DD_CLIENT_TOKEN
ARG VITE_DD_APPLICATION_ID
ARG VITE_DD_ENV=production
ARG VITE_DD_SERVICE=rkgw-frontend
ARG VITE_DD_SITE=datadoghq.com
RUN npm run build

# Runtime stage
FROM nginx:1.27-alpine
COPY --from=builder /build/dist /usr/share/nginx/html
COPY nginx.conf.template /etc/nginx/templates/default.conf.template
RUN mkdir -p /var/www/certbot
EXPOSE 80 443
