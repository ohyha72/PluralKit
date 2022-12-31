FROM mcr.microsoft.com/dotnet/sdk:6.0 as build

WORKDIR /app

COPY .git/ /app/.git
COPY proto/ /app/proto

COPY services/gateway_worker/ /app/services/gateway_worker
COPY Myriad/ /app/Myriad
COPY PluralKit.Core/ /app/PluralKit.Core

RUN dotnet build services/gateway_worker -c Release -o bin

# Build runtime stage (doesn't include SDK)
FROM mcr.microsoft.com/dotnet/aspnet:6.0

WORKDIR /app
COPY --from=build /app/bin ./

ENTRYPOINT ["/app/GatewayWorker"]
