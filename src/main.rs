//! Проект предназначен для разработки “подсистемы обработки” (далее в тексте - модуль) в терминах описания
//! общей архитектуры программно-аппаратного комплекса.
//! Основным входным каналом данных является TCP-подключение к подсистеме сопряжения.
//! По данному каналу подсистема сопряжения передает, а модуль получает входные данные по специальному V-протоколу, описанному в отдельном документе.
//! Общее описание V-протокола дано в описании верхнего уровня комплекса.
//! Основной выходной канал - это канал выдачи обработанных данных после отработки TensorRT для дальнейшего сохранения.
//! 
//! Структурно проект представляет собой несколько компонентов, совместная работа которых обеспечивает решение задачи:
//! *  input - подключается к заданным экземплярам системы сопряжения и получает от них на вход поток смешанных данных
//! *  collector - собирает из полученных смешанных фрагментов звуковые сессии для дальнейшей обработки
//! *  processor - выполняет обработку собранных звуковых сессий, получая готовые звуковые сэмплы
//! *  inference - вычисляет сохраняемый результат для каждого полученного сэмпла
//! *  output - отправляет на сохранение полученный результат
//! *  logging - вспомогательная подсистема логирования работы приложения
//! *  config - вспомогательная  подсистема централизованного доступа к настройкам приложения
//! *  data - модуль определения структур данных для обмена между подсистемами
//! 
//! Реализация приложения основана на событийно-асинхронной модели на базе фреймворка tokio 

mod logger;
mod data;
mod input;
mod collector;
mod processor;
mod inference;
mod output;
mod config;
use config::Config;

use tokio::sync::oneshot;
use tokio::sync::mpsc::channel;

/// Осуществляет предварительную настройку и запуск подсистем приложения
/// *  создание подмодуля конфигурации
/// *  настройку сервиса логирования
/// *  создание каналов обмена между основными подсистемами
/// *  запуск основных подсистем - input, collector, processor, inference, output
/// *  запуск обработчика системных сигналов для корректного завершения приложения
#[tokio::main]
async fn main() {

    println!("Hello, banshee");

    // instantiate config
    let cfg_inst = Config::new();

    // init logger
    logger::init(cfg_inst.clone());
    // now logging is available

    let subsystems = async move {
        // channel to control input is a oneshot
        let (tx_stop, rx_stop) = oneshot::channel();
        
        // other channels are universal        
        // channel to pass fragments: input --> collector
        let (mut tx_frag, rx_frag) = channel::<data::Fragment>(100);
        // channel to pass sessions: collector --> processor
        let (mut tx_sess, rx_sess) = channel::<data::Session>(100);
        // channel to pass samples: procesor --> inference
        let (mut tx_smpl, rx_smpl) = channel::<data::FinalSample>(100);
        // channel to pass stored results: inference --> output
        let (mut tx_rslt, rx_rslt) = channel::<data::StoredResult>(100);

        // launch worker submodules
        input::run(cfg_inst.clone(), rx_stop, tx_frag.clone()).await;
        collector::run(cfg_inst.clone(), rx_frag, tx_sess.clone()).await;
        processor::run(cfg_inst.clone(), rx_sess, tx_smpl.clone()).await;
        inference::run(cfg_inst.clone(), rx_smpl, tx_rslt.clone()).await;
        output::run(cfg_inst.clone(), rx_rslt).await;

        // launch stop handler, <Enter> in Windows, <Ctrl+C> in others
        let signals = platform::get_system_signals();
        tokio::spawn(async move {
            for _ in signals.forever() {
                println!("\nTrying to stop banshee!\n");
                // send stop signal to all channels
                let _ = tx_stop.send(());                               // stops input
                let _ = tx_frag.send(data::Fragment::Stop).await;       // stops collector
                let _ = tx_sess.send(data::Session::Stop).await;        // stops processor
                let _ = tx_smpl.send(data::FinalSample::Stop).await;    // stops inference
                let _ = tx_rslt.send(data::StoredResult::Stop).await;   // stops output
                break;
            }
        }).await.unwrap();
    };

    subsystems.await;
}

/// Non-windows реализация обработчика Ctrl-C (SIGINT)
#[cfg(not(windows))]
mod platform {
    use signal_hook::{iterator::Signals, SIGINT};
    
    /// Возвращает способный итерироваться (т.е. срабатывать) по каждому сигналу SIGINT объект (non-Windows)
    /// Срабатывание реализовано в виде итератора, который возвращает одно значение на каждый сигнал
    pub fn get_system_signals() -> Signals {
        println!("Banshee has started, press Ctrl+C to stop");
        Signals::new(&[SIGINT]).unwrap()
    }
}

/// Совместимая с обработчиком Ctrl-C (SIGINT) для linux реализация для windows, реагируюшая на <Enter>
#[cfg(windows)]
mod platform {
    use std::io::BufRead;

    /// Способный однократно срабатывать по нажатию Enter объект (Windows only).
    pub struct Signals {}

    impl Signals {

        /// Возвращает разовый итератор, срабатывающий по нажатию Enter в консоли запущенной программы
        pub fn forever(&self) -> std::iter::Once<()> {
            println!("Banshee has started, press Enter to stop");
            let stdin = std::io::stdin();
            for _ in stdin.lock().lines() {
                break;
            }
            std::iter::once(())
        }

    }

    /// Возвращает способный итерироваться (т.е. срабатывать) по нажатию <Enter> объект (for Windows only)
    /// Срабатывание реализовано в виде разового итератора, который возвращает значение по нажатию Enter
    pub fn get_system_signals() -> Signals {
        Signals {}
    }
}
