project(
  'renovatio',
  'rust',
  version: '0.1',
  default_options: ['warning_level=3', 'rust_std=2024'],

)

# i18n = import('i18n')
gnome = import('gnome')

subdir('src')

install_data(
  'data/com.github.AdamIsrael.Renovatio.gschema.xml',
  install_dir: get_option('datadir') / 'glib-2.0' / 'schemas',
)

gnome.post_install(
  glib_compile_schemas: true,
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
