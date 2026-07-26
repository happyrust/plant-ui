#!/bin/bash

# Set default values for environment variables if not defined
: "${HTTP_ADDR:=http://127.0.0.1:8009}"
: "${DATABASE_NAME:=AvevaMarineSample}"
: "${NAMESPACE:=1516}"

# Define surql files to process
surql_files="common.surql"

# Process each surql file
for file in $surql_files; do
    surreal import --conn "$HTTP_ADDR" \
                  --namespace "$NAMESPACE" \
                  --database "$DATABASE_NAME" \
                  -u root -p root "$file"
done 