use std::{
  thread, thread::JoinHandle,
  sync::{Arc,Mutex}
};
use std::sync::mpsc;
use super::ThreadPool;
use crate::Result;


/// 定义Job的类型，Job应该是实现了 Fnonce+Send+'static 的类型，以保证多线程的安全性
type Job = Box<dyn FnOnce() + Send + 'static>;


/// Worker结构体，包含了ID和JoinHandle，是线程池工作的基本单位
pub struct Worker{
    /// id:每个工作线程的唯一标识符
    id: usize,

    /// thread: 使用Option封装的JoinHandle
    thread: Option<JoinHandle<()>>,

}

impl Worker{
    /// 线程启动并争抢唯一的 Receiver
    fn new(id: usize,receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker{
        let thread = thread::spawn(move ||{
            loop{
                let message = receiver.lock().unwrap().recv();
                match message {
                    Ok(job) => {
                        job();
                    }
                    Err(_) => {
                        // 说明此时Receiver 被销毁，退出循环。
                        break;
                    }
                }
            }
        });
        Worker { id, thread: Some(thread) }
    }
}




/// NaiveThreadPool是线程池的主结构体
pub struct NaiveThreadPool{
    /// 线程数组,即消费者列表
    workers: Vec<Worker>,

    /// 发送器：任务队列的入口
    sender: Option<mpsc::Sender<Job>>,
}


impl ThreadPool for NaiveThreadPool{
    /// 创建一个新的线程池，初始化mpsc和线程
    fn new(threads: u32) -> Result<Self>
        where
            Self: Sized {
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(threads as usize);
        for id in 0..threads{
            let receiver_clone = Arc::clone(&receiver);
            workers.push(Worker::new(id as usize,receiver_clone));
        }
        Ok(NaiveThreadPool { workers, sender: Some(sender) })
    }

    /// 将任务发送到线程池进行执行
    fn spawn<F>(&self, job: F)
        where
            F: FnOnce() + Send + 'static {
        self.sender.as_ref().unwrap().send(Box::new(job)).unwrap();
    }
}

impl Drop for NaiveThreadPool {
    fn drop(&mut self) {
        // Move Sender from self
        drop(self.sender.take());
        // 2. Wait for workers to finish
        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}