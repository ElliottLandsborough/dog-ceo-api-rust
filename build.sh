git clone --depth 1 --single-branch https://github.com/jigsawpieces/dog-api-images.git tempimages
rm -rf tempimages/.git
rm -rf tempimages/.gitignore
rm -rf tempimages/README.md
rm -rf tempimages/LICENSE
docker build -t dog-ceo-api-rust:runtime .
rm -rf tempimages